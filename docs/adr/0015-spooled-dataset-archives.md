# Spooled dataset archives and streamed transfers

Frozen snapshots, dataset exports and snapshot downloads all hand out one ZIP whose members are
the resampled LoRA samples of the curated library. The archive was built in a `Cursor<Vec<u8>>`,
hashed in place, uploaded from that buffer, and every download path read it back into another
`Vec` — so one freeze held the whole archive (multiple gigabytes for a few hundred maps) plus
64 MiB of upload parts resident, and three routes held it again per request. The pod's 1536 MiB limit made a
multi-gigabyte archive impossible to build on the cluster, which is why freezes ran on a workstation.

The archive now spools to `DATA_DIR/tmp/dataset-archive-<pid>-<n>.zip`, a hard-linked-by-name
temporary file that deletes itself on drop, and moves in both directions as a stream:

- **Build**: `package_lora_zip_streaming` writes into an `ArchiveWriter`, a `Write + Seek` handle
  that fails with `FileTooLarge` once the spool would exceed `CURATOR_MAX_ARCHIVE_BYTES`
  (default 8 GiB). Members use `Stored`, because PNGs are already compressed and Deflate cost a
  full pass for ~0–2 %.
- **Digest**: the finished file is hashed with one sequential read. The digest is object metadata
  for the snapshot upload and the record's `sha256`; it never required the archive to be in RAM.
- **Upload**: `BlobStorage::write_blob_stream` takes an `AsyncRead` and `total_bytes`. S3 reads
  16 MiB parts from the file, four in flight, exactly as the byte-array path did; the filesystem
  adapter copies to a sibling temp file and renames. A short or long stream aborts the upload
  instead of storing an object whose metadata disagrees with its bytes.
- **Download**: `BlobStorage::read_blob_stream` returns an `AsyncRead`. The snapshot download
  streams the object straight from S3; the two export routes stream their spool file. All three
  set `Content-Length` from the known size and never buffer the archive.
- **Concurrency**: one process-wide spool slot. A second freeze or export waits for the first
  archive to be uploaded or fully streamed out, because two spools on one volume is what turns a
  large dataset into a full disk. Snapshot downloads bypass the slot: they stream from storage and
  never write a spool.

## Considered Options

- **Keep the in-memory `Vec`, raise the pod memory limit vs. spool to disk**: the archive is
  proportional to the library, which grows, and the memory limit would have to grow with it.
  Spooling bounds resident memory to one resampled section regardless of dataset size.
- **Stream directly from the ZIP writer into multipart parts vs. materialize first**: a pipe would
  remove the temp file, but it forces chunked uploads with no retry, no content length for the
  digest, and an abort path that races the writer. The spool costs one extra sequential read and
  buys a complete, re-readable artifact.
- **`Stored` vs. `Deflate` members**: Deflate on PNG data returns a few percent for a full pass
  over gigabytes. `Stored` makes the spool size predictable and the writer cheaper; the reader
  handles both, so existing snapshots keep working.
- **No archive ceiling vs. `CURATOR_MAX_ARCHIVE_BYTES`**: without a ceiling a runaway scope fills
  the volume and takes the catalog and telemetry down with it. The ceiling fails the one operation
  with a message naming the variable.
- **A single fixed spool filename vs. unique names**: a fixed name cannot be safely reused while a
  previous upload still reads it, and leaves the previous archive unrecoverable after a crash.
  Unique names plus a boot sweep of files older than a day cover both.
- **Sweeping at boot only vs. per-request scans**: the spool is written by one operation at a time;
  a scan on every request would cost more than the stale file it might find.

## Consequences

- Freeze RSS no longer scales with the archive; the pod-level constraint that forced local runs is
  gone for snapshots and exports. Resident cost is the archive writer's buffer plus one decoded
  Ground layer plus one resampled section.
- The volume now needs room for one archive (default cap 8 GiB) alongside the data directory. With
  the filesystem blob backend that is the same 20 GiB volume, so an oversized scope fails with
  `CURATOR_MAX_ARCHIVE_BYTES` rather than an OS error at an arbitrary point.
- An archive that exceeds the ceiling aborts mid-write: the partial spool is deleted and no
  snapshot record is written.
- A dropped upload future now aborts its S3 multipart upload from a `Drop` guard instead of
  leaving billable parts behind. The lifecycle rule remains the fallback when the process dies
  outright.
- Exports and snapshot downloads respond only after the archive is complete, as before; the change
  removes the server-side buffer, not the client-visible wait.
- Spool files are derived and safe to delete; a stale one from a crash is swept at the next boot.
