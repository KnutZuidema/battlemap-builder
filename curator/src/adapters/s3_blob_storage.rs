fn listed_size(obj: &aws_sdk_s3::types::Object) -> u64 {
    obj.size().map_or(0, |size| size.max(0) as u64)
}
