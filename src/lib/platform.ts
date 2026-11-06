// Which system Replix runs on, for the few words that differ (the recycle bin is the Trash
// on macOS and Linux) and for paths. The web view says it in its user agent: WebView2 on
// Windows, WebKit on macOS and Linux.

const agent = typeof navigator === "undefined" ? "" : navigator.userAgent;

export const isWindows = /Windows/i.test(agent);
export const isMac = !isWindows && /Macintosh|Mac OS X/i.test(agent);
export const isLinux = !isWindows && !isMac;

/** A path with `/` for separators and no separator at the end, for comparing paths. */
export function normalPath(path: string): string {
  const slashed = path.replace(/\\/g, "/").replace(/\/+$/, "");
  // Windows paths ignore case; the others do not.
  return isWindows ? slashed.toLowerCase() : slashed;
}

/** Whether `path` is `folder` or lies inside it. */
export function isInside(path: string, folder: string): boolean {
  const root = normalPath(folder);
  return root !== "" && (normalPath(path) + "/").startsWith(root + "/");
}

/**
 * Whether a program's full path ends in this executable name, with either separator.
 * Ignores case, as Replix compares executable names everywhere.
 */
export function endsWithExe(path: string, exe: string): boolean {
  const p = path.replace(/\\/g, "/").toLowerCase();
  const name = exe.toLowerCase();
  return p === name || p.endsWith("/" + name);
}
