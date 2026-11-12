// What the page itself can play. The editor shows a clip in the page, so a format Replix
// records in has to be one the page can decode too.

/** Whether the page can play HEVC from an MP4 (WebView2 and WebKitGTK need the system's decoder for it). */
export function playsHevc(): boolean {
  try {
    return document.createElement("video").canPlayType('video/mp4; codecs="hvc1.1.6.L150.90"') !== "";
  } catch {
    return false;
  }
}
