import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export type UpdatePhase = "idle" | "checking" | "current" | "available" | "downloading" | "failed";

const EVERY = 6 * 60 * 60 * 1000;

/** New versions come from the latest GitHub release, signed with the key in tauri.conf.json. */
class Updates {
  version = $state("");
  phase = $state<UpdatePhase>("idle");
  next = $state<Update | null>(null);
  /** Download progress from 0 to 1, or null while the size is unknown. */
  progress = $state<number | null>(null);
  private announced = "";

  async init(announce: (version: string) => void) {
    this.version = await getVersion().catch(() => "");
    // A dev build has no release to compare against.
    if (import.meta.env.DEV) return;
    const run = async () => {
      if ((await this.check()) && this.next && this.next.version !== this.announced) {
        this.announced = this.next.version;
        announce(this.next.version);
      }
    };
    setTimeout(run, 8000);
    setInterval(run, EVERY);
  }

  /** Whether a newer version is waiting. */
  async check(): Promise<boolean> {
    if (this.phase === "checking" || this.phase === "downloading") return false;
    this.phase = "checking";
    try {
      this.next = await check();
      this.phase = this.next ? "available" : "current";
    } catch (e) {
      console.warn("update check failed", e);
      this.phase = "failed";
    }
    return this.phase === "available";
  }

  async install() {
    if (!this.next || this.phase === "downloading") return;
    this.phase = "downloading";
    this.progress = null;
    let total = 0;
    let received = 0;
    try {
      await this.next.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          received += event.data.chunkLength;
          this.progress = total ? received / total : null;
        }
      });
      // Windows closes the app for the installer; on macOS and Linux start the new version.
      await relaunch();
    } catch (e) {
      console.warn("update failed", e);
      this.phase = "failed";
    }
  }
}

export const updates = new Updates();
