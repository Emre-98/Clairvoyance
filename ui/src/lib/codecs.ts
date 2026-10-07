// v1.8: which video codecs the in-app player (WebView2 / Chromium) can play. Reported to the
// app at start-up: the recorder only records HEVC or AV1 when the player can play them (and
// Windows can decode them for thumbnails and the ult check), H.264 otherwise.

export const CODEC_TYPES: Record<string, string> = {
  h264: 'video/mp4; codecs="avc1.640028"',
  hevc: 'video/mp4; codecs="hvc1.1.6.L123.B0"',
  av1: 'video/mp4; codecs="av01.0.08M.08"',
};

export function playableCodecs(probe?: (type: string) => string): string[] {
  const can =
    probe ??
    ((t: string) => {
      try {
        return document.createElement("video").canPlayType(t);
      } catch {
        return "";
      }
    });
  return Object.entries(CODEC_TYPES)
    .filter(([, t]) => can(t) !== "")
    .map(([c]) => c);
}

export const sameList = (a: string[] | undefined, b: string[]) => !!a && a.length === b.length && a.every((x, i) => x === b[i]);
