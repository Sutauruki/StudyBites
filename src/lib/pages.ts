import type { CardSource, SourceRef } from "../api/types";

/**
 * "p. 3", "pp. 3-5", or "" when there is no page to name.
 *
 * Empty is the honest answer for a DOCX, a text file, or a note made before
 * page references existed. The UI shows the material name alone rather than
 * inventing a page the student would go to and find the wrong thing on.
 */
export function formatPages(start: number | null, end: number | null): string {
  if (start === null) return "";
  const last = end === null || end < start ? start : end;
  return last === start ? `p. ${start}` : `pp. ${start}-${last}`;
}

/**
 * The one muted line under a card: "Source: Biology.pdf, pp. 3-5".
 *
 * Returns null when nothing is known, so the caller renders no line at all
 * rather than a label with nothing after it.
 */
export function sourceLine(source: CardSource | null | undefined): string | null {
  if (!source) return null;
  if (source.isPreview) return "AI preview, not from your material";
  const pages = formatPages(source.pageStart, source.pageEnd);
  if (source.materialName) {
    return pages ? `Source: ${source.materialName}, ${pages}` : `Source: ${source.materialName}`;
  }
  return pages ? `Source: ${pages}` : null;
}

/**
 * One "From your notes" chip: "Cell division, Biology.pdf, pp. 3-5".
 *
 * The note's own title comes first because that is what the student
 * recognises; the file and page are there to check it.
 */
export function citationLabel(ref: SourceRef): string {
  if (ref.isPreview) return `${ref.title} (AI preview, not from your material)`;
  const pages = formatPages(ref.pageStart, ref.pageEnd);
  return [ref.title, ref.materialName, pages].filter((part) => part).join(", ");
}
