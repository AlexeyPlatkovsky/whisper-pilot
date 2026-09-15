import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

const LOCAL_IMPORT = /@import\s+(?:url\(\s*)?["']([^"']+)["']\s*\)?\s*;/g;

/**
 * Resolves the local stylesheet entry point for assertions that describe the
 * rendered CSS contract. CSS is split into modules, so reading styles.css by
 * itself would silently omit rules imported by that entry point.
 */
export function readCssBundle(
  entryPath = resolve(process.cwd(), "src/styles.css"),
): string {
  const visited = new Set<string>();

  const readBundle = (filePath: string): string => {
    const absolutePath = resolve(filePath);
    if (visited.has(absolutePath)) {
      return "";
    }
    visited.add(absolutePath);

    return readFileSync(absolutePath, "utf8").replace(
      LOCAL_IMPORT,
      (statement, specifier: string) => {
        if (!specifier.startsWith(".")) {
          return statement;
        }
        return readBundle(resolve(dirname(absolutePath), specifier));
      },
    );
  };

  return readBundle(entryPath);
}
