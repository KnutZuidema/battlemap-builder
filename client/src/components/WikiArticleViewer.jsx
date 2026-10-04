import React, { useState } from 'react';
import {
  Quote,
  Eye,
  Lock,
  Info,
  Lightbulb,
  AlertTriangle,
  Copy,
  Check,
  MapPin,
  ExternalLink
} from 'lucide-react';

/**
 * Obsidian callout theme config
 */
const CALLOUT_STYLES = {
  description: {
    label: 'Read Aloud',
    icon: Quote,
    card: 'bg-amber-500/10 border-amber-500/30 text-amber-100',
    title: 'text-amber-300 font-semibold',
    quote: 'italic text-amber-100/90 text-sm leading-relaxed',
    btn: 'text-amber-300 hover:text-amber-100 bg-amber-500/15 hover:bg-amber-500/25 border border-amber-500/30'
  },
  secret: {
    label: 'DM Secret / Info',
    icon: Lock,
    card: 'bg-purple-950/40 border-purple-500/35 text-purple-100',
    title: 'text-purple-300 font-semibold',
    quote: 'text-purple-100/90 text-sm leading-relaxed',
    btn: 'text-purple-300 hover:text-purple-100 bg-purple-500/15 hover:bg-purple-500/25 border border-purple-500/30'
  },
  tip: {
    label: 'Tip',
    icon: Lightbulb,
    card: 'bg-emerald-950/40 border-emerald-500/35 text-emerald-100',
    title: 'text-emerald-300 font-semibold',
    quote: 'text-emerald-100/90 text-sm leading-relaxed',
    btn: 'text-emerald-300 hover:text-emerald-100 bg-emerald-500/15 hover:bg-emerald-500/25 border border-emerald-500/30'
  },
  note: {
    label: 'Note',
    icon: Info,
    card: 'bg-cyan-950/40 border-cyan-500/35 text-cyan-100',
    title: 'text-cyan-300 font-semibold',
    quote: 'text-cyan-100/90 text-sm leading-relaxed',
    btn: 'text-cyan-300 hover:text-cyan-100 bg-cyan-500/15 hover:bg-cyan-500/25 border border-cyan-500/30'
  },
  warning: {
    label: 'Warning',
    icon: AlertTriangle,
    card: 'bg-rose-950/40 border-rose-500/35 text-rose-100',
    title: 'text-rose-300 font-semibold',
    quote: 'text-rose-100/90 text-sm leading-relaxed',
    btn: 'text-rose-300 hover:text-rose-100 bg-rose-500/15 hover:bg-rose-500/25 border border-rose-500/30'
  }
};

function getCalloutStyle(type) {
  const key = (type || '').toLowerCase();
  if (CALLOUT_STYLES[key]) return { ...CALLOUT_STYLES[key], key };
  if (key.includes('desc') || key.includes('read') || key.includes('aloud')) return { ...CALLOUT_STYLES.description, key: 'description' };
  if (key.includes('secret') || key.includes('dm')) return { ...CALLOUT_STYLES.secret, key: 'secret' };
  if (key.includes('tip')) return { ...CALLOUT_STYLES.tip, key: 'tip' };
  if (key.includes('warn') || key.includes('danger')) return { ...CALLOUT_STYLES.warning, key: 'warning' };
  return { ...CALLOUT_STYLES.note, key: 'note' };
}

/**
 * Render inline markdown elements: bold, italic, wikilinks, code, tags
 */
export function renderInlineMarkdown(text, { onNavigateArticle } = {}) {
  if (!text) return null;

  // Split tokens: wikilinks [[Link|Alias]], images ![[img]], bold **text**, code `text`
  const tokenRegex = /(\!\[\[[^\]]+\]\]|\[\[[^\]]+\]\]|\*\*[^*]+\*\*|\*[^*]+\*|`[^`]+`)/g;
  const parts = text.split(tokenRegex);

  return parts.map((part, index) => {
    if (!part) return null;

    // Obsidian Embed ![[image.jpg]]
    if (part.startsWith('![[') && part.endsWith(']]')) {
      const target = part.slice(3, -2);
      return (
        <span
          key={index}
          className="inline-flex items-center gap-1.5 px-2 py-0.5 mx-1 text-xs font-medium text-slate-400 bg-white/5 border border-white/10 rounded-md"
        >
          <MapPin size={11} className="text-amber-400/80" />
          <span>{target}</span>
        </span>
      );
    }

    // WikiLink [[Target|Alias]] or [[Target]]
    if (part.startsWith('[[') && part.endsWith(']]')) {
      const inner = part.slice(2, -2);
      let target = inner;
      let label = inner;
      if (inner.includes('|')) {
        const split = inner.split('|');
        target = split[0].trim();
        label = split[1].trim();
      } else {
        // use last segment of path as label if nested
        const segs = target.split('/');
        label = segs[segs.length - 1];
      }

      // Check if target points to a wiki article path
      const isResolvable = target.includes('/') || target.endsWith('.md');
      const clickHandler = onNavigateArticle ? () => onNavigateArticle(target) : undefined;

      return (
        <button
          key={index}
          type="button"
          onClick={clickHandler}
          disabled={!onNavigateArticle}
          className={`inline-flex items-center gap-1 px-1.5 py-0.5 mx-0.5 text-xs font-semibold rounded transition-colors ${
            onNavigateArticle
              ? 'text-cyan-300 hover:text-cyan-100 bg-cyan-500/10 hover:bg-cyan-500/20 border border-cyan-500/25 cursor-pointer'
              : 'text-cyan-300 bg-cyan-500/10 border border-cyan-500/20'
          }`}
          title={`Wiki: ${target}`}
        >
          <span>{label}</span>
          {onNavigateArticle && <ExternalLink size={9} className="opacity-60" />}
        </button>
      );
    }

    // Bold **text**
    if (part.startsWith('**') && part.endsWith('**') && part.length >= 4) {
      return (
        <strong key={index} className="font-semibold text-white">
          {part.slice(2, -2)}
        </strong>
      );
    }

    // Italic *text*
    if (part.startsWith('*') && part.endsWith('*') && part.length >= 2) {
      return (
        <em key={index} className="italic text-slate-200">
          {part.slice(1, -1)}
        </em>
      );
    }

    // Inline Code `text`
    if (part.startsWith('`') && part.endsWith('`') && part.length >= 2) {
      return (
        <code
          key={index}
          className="font-mono text-xs px-1.5 py-0.5 mx-0.5 bg-black/40 text-amber-300 rounded border border-white/10"
        >
          {part.slice(1, -1)}
        </code>
      );
    }

    return <span key={index}>{part}</span>;
  });
}

/**
 * Single Callout Card with Copy Read-Aloud action
 */
function CalloutCard({ type, customTitle, lines, onNavigateArticle }) {
  const [copied, setCopied] = useState(false);
  const style = getCalloutStyle(type);
  const Icon = style.icon;
  const rawText = lines.join('\n');

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(rawText);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy read-aloud:', err);
    }
  };

  const titleText = customTitle || style.label;

  return (
    <div className={`my-4 p-4 rounded-xl border backdrop-blur-md transition-all ${style.card}`}>
      <div className="flex items-center justify-between gap-3 mb-2.5">
        <div className="flex items-center gap-2">
          <div className="p-1 rounded-md bg-white/10">
            <Icon size={15} />
          </div>
          <span className={`text-xs uppercase tracking-wider ${style.title}`}>
            {titleText}
          </span>
        </div>

        {style.key === 'description' && (
          <button
            type="button"
            onClick={handleCopy}
            className={`inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-lg transition-all ${style.btn}`}
            title="Copy read-aloud description for DM table"
          >
            {copied ? (
              <>
                <Check size={12} className="text-emerald-400" />
                <span className="text-emerald-300">Copied!</span>
              </>
            ) : (
              <>
                <Copy size={12} />
                <span>Copy Read-Aloud</span>
              </>
            )}
          </button>
        )}
      </div>

      <div className={style.quote}>
        {lines.map((line, idx) => (
          <p key={idx} className={idx > 0 ? 'mt-2' : ''}>
            {renderInlineMarkdown(line, { onNavigateArticle })}
          </p>
        ))}
      </div>
    </div>
  );
}

/**
 * Lightweight Table component
 */
function MarkdownTable({ headers, rows, onNavigateArticle }) {
  return (
    <div className="my-4 overflow-x-auto rounded-xl border border-white/10 bg-black/25">
      <table className="w-full text-left text-xs border-collapse">
        <thead>
          <tr className="border-b border-white/10 bg-white/5">
            {headers.map((h, i) => (
              <th key={i} className="py-2.5 px-3.5 font-semibold text-slate-200">
                {renderInlineMarkdown(h, { onNavigateArticle })}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-white/5">
          {rows.map((row, rIdx) => (
            <tr key={rIdx} className="hover:bg-white/[0.02] transition-colors">
              {row.map((cell, cIdx) => (
                <td key={cIdx} className="py-2 px-3.5 text-slate-300">
                  {renderInlineMarkdown(cell, { onNavigateArticle })}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/**
 * Main Obsidian / Wiki markdown article viewer component
 */
export default function WikiArticleViewer({ article, onNavigateArticle }) {
  if (!article) {
    return (
      <div className="p-8 text-center text-slate-500 text-sm">
        No article selected.
      </div>
    );
  }

  const { title, content, parentRegion } = article;

  // Split lines and parse blocks
  const lines = (content || '').split(/\r?\n/);
  const blocks = [];

  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const trimmed = line.trim();

    // 1. Skip frontmatter
    if (i === 0 && trimmed === '---') {
      i++;
      while (i < lines.length && lines[i].trim() !== '---') {
        i++;
      }
      i++;
      continue;
    }

    // 2. Empty line
    if (!trimmed) {
      i++;
      continue;
    }

    // 3. Horizontal Rule
    if (/^(\-{3,}|\*{3,}|_{3,})$/.test(trimmed)) {
      blocks.push({ type: 'hr' });
      i++;
      continue;
    }

    // 4. Obsidian Callout or Blockquote: starts with >
    if (trimmed.startsWith('>')) {
      const calloutLines = [];
      let isCallout = false;
      let calloutType = '';
      let calloutTitle = '';

      // Check first line for [!type] Title
      const firstLineContent = trimmed.replace(/^>\s?/, '');
      const calloutMatch = firstLineContent.match(/^\[!([a-zA-Z0-9_\-]+)\]\s*(.*)$/);

      if (calloutMatch) {
        isCallout = true;
        calloutType = calloutMatch[1];
        calloutTitle = calloutMatch[2].trim();
      } else {
        calloutLines.push(firstLineContent);
      }

      i++;
      while (i < lines.length && lines[i].trim().startsWith('>')) {
        calloutLines.push(lines[i].trim().replace(/^>\s?/, ''));
        i++;
      }

      if (isCallout) {
        blocks.push({
          type: 'callout',
          calloutType,
          calloutTitle,
          lines: calloutLines
        });
      } else {
        blocks.push({
          type: 'blockquote',
          lines: calloutLines
        });
      }
      continue;
    }

    // 5. Code Block: ```
    if (trimmed.startsWith('```')) {
      const lang = trimmed.slice(3).trim();
      const codeLines = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith('```')) {
        codeLines.push(lines[i]);
        i++;
      }
      i++; // skip closing ```
      blocks.push({
        type: 'code',
        lang,
        code: codeLines.join('\n')
      });
      continue;
    }

    // 6. Markdown Table: starts with | and next row has | :--- |
    if (trimmed.startsWith('|') && trimmed.endsWith('|')) {
      const tableLines = [trimmed];
      i++;
      while (i < lines.length && lines[i].trim().startsWith('|') && lines[i].trim().endsWith('|')) {
        tableLines.push(lines[i].trim());
        i++;
      }

      if (tableLines.length >= 2) {
        const parseRow = (r) =>
          r
            .slice(1, -1)
            .split('|')
            .map((c) => c.trim());
        const headers = parseRow(tableLines[0]);
        // line 1 is separator | :--- |
        const rows = tableLines.slice(2).map(parseRow);
        blocks.push({
          type: 'table',
          headers,
          rows
        });
        continue;
      }
    }

    // 7. Headings: # Heading
    const headingMatch = trimmed.match(/^(#{1,6})\s+(.+)$/);
    if (headingMatch) {
      blocks.push({
        type: 'heading',
        level: headingMatch[1].length,
        text: headingMatch[2]
      });
      i++;
      continue;
    }

    // 8. Lists: * item or - item or 1. item
    if (/^(\*|\-|\d+\.)\s+/.test(trimmed)) {
      const listItems = [];
      while (i < lines.length && /^(\*|\-|\d+\.)\s+/.test(lines[i].trim())) {
        const itemText = lines[i].trim().replace(/^(\*|\-|\d+\.)\s+/, '');
        listItems.push(itemText);
        i++;
      }
      blocks.push({
        type: 'list',
        items: listItems
      });
      continue;
    }

    // 9. Standard Paragraph
    const paraLines = [line];
    i++;
    while (
      i < lines.length &&
      lines[i].trim() &&
      !lines[i].trim().startsWith('#') &&
      !lines[i].trim().startsWith('>') &&
      !lines[i].trim().startsWith('```') &&
      !lines[i].trim().startsWith('|') &&
      !/^(\*|\-|\d+\.)\s+/.test(lines[i].trim())
    ) {
      paraLines.push(lines[i]);
      i++;
    }

    blocks.push({
      type: 'paragraph',
      text: paraLines.join(' ')
    });
  }

  return (
    <div className="wiki-article-viewer text-slate-300 text-sm leading-relaxed space-y-4">
      {/* Article Header */}
      <div className="pb-4 mb-4 border-b border-white/10">
        <h1 className="text-xl sm:text-2xl font-bold font-display text-white tracking-tight">
          {title}
        </h1>
        {parentRegion && (
          <div className="flex items-center gap-2 mt-2 text-xs text-amber-400/90 font-medium">
            <MapPin size={13} className="text-amber-400" />
            <span>Region: {parentRegion.name}</span>
          </div>
        )}
      </div>

      {/* Render Parsed Blocks */}
      {blocks.map((block, idx) => {
        switch (block.type) {
          case 'heading': {
            if (block.level === 1) {
              return (
                <h2
                  key={idx}
                  className="text-lg font-bold font-display text-white mt-6 mb-2 pb-1 border-b border-white/10"
                >
                  {renderInlineMarkdown(block.text, { onNavigateArticle })}
                </h2>
              );
            }
            if (block.level === 2) {
              return (
                <h3
                  key={idx}
                  className="text-base font-semibold font-display text-amber-200 mt-5 mb-2"
                >
                  {renderInlineMarkdown(block.text, { onNavigateArticle })}
                </h3>
              );
            }
            return (
              <h4
                key={idx}
                className="text-sm font-semibold text-slate-200 mt-4 mb-1 tracking-wide"
              >
                {renderInlineMarkdown(block.text, { onNavigateArticle })}
              </h4>
            );
          }

          case 'paragraph': {
            return (
              <p key={idx} className="text-slate-300 leading-relaxed">
                {renderInlineMarkdown(block.text, { onNavigateArticle })}
              </p>
            );
          }

          case 'callout': {
            return (
              <CalloutCard
                key={idx}
                type={block.calloutType}
                customTitle={block.calloutTitle}
                lines={block.lines}
                onNavigateArticle={onNavigateArticle}
              />
            );
          }

          case 'blockquote': {
            return (
              <blockquote
                key={idx}
                className="my-3 pl-3 py-1 border-l-2 border-slate-600 italic text-slate-400 text-sm"
              >
                {block.lines.map((l, lIdx) => (
                  <p key={lIdx}>{renderInlineMarkdown(l, { onNavigateArticle })}</p>
                ))}
              </blockquote>
            );
          }

          case 'list': {
            return (
              <ul key={idx} className="my-2 space-y-1.5 pl-4 list-disc marker:text-amber-400/60">
                {block.items.map((item, iIdx) => (
                  <li key={iIdx} className="text-slate-300">
                    {renderInlineMarkdown(item, { onNavigateArticle })}
                  </li>
                ))}
              </ul>
            );
          }

          case 'table': {
            return (
              <MarkdownTable
                key={idx}
                headers={block.headers}
                rows={block.rows}
                onNavigateArticle={onNavigateArticle}
              />
            );
          }

          case 'code': {
            return (
              <div
                key={idx}
                className="my-3 p-3 rounded-xl bg-black/40 border border-white/10 font-mono text-xs overflow-x-auto text-emerald-300/90"
              >
                <div className="text-[10px] uppercase text-slate-500 mb-1">{block.lang || 'code'}</div>
                <pre className="whitespace-pre">{block.code}</pre>
              </div>
            );
          }

          case 'hr': {
            return <hr key={idx} className="my-6 border-white/10" />;
          }

          default:
            return null;
        }
      })}
    </div>
  );
}
