// Upload links per file: which are on their way and which are ready to copy again.
// Kept across restarts until they run out.

const KEY = "replix.links";
const LIFETIME = 72 * 3600_000;

export interface Link {
  url: string;
  /** When the link stops working, in ms. */
  until: number;
}

function load(): Record<string, Link> {
  try {
    const all = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Record<string, Link>;
    return Object.fromEntries(Object.entries(all).filter(([, l]) => l.until > Date.now()));
  } catch {
    return {};
  }
}

class Links {
  ready = $state<Record<string, Link>>(load());
  uploading = $state<Record<string, true>>({});

  /** The link for a file while it still works. */
  of(path: string): Link | null {
    const link = this.ready[path];
    return link && link.until > Date.now() ? link : null;
  }

  add(path: string, url: string) {
    this.ready[path] = { url, until: Date.now() + LIFETIME };
    try {
      localStorage.setItem(KEY, JSON.stringify(this.ready));
    } catch {
      // Only a convenience; the link was copied either way.
    }
  }
}

export const links = new Links();

/** "71 h" or "40 min" left. */
export function remaining(link: Link): string {
  const mins = Math.max(0, Math.round((link.until - Date.now()) / 60_000));
  return mins >= 60 ? `${Math.floor(mins / 60)} h` : `${mins} min`;
}
