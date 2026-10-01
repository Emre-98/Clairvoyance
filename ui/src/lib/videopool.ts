// One <video> element for the whole app. It's moved into the player when a game page opens and
// kept (with its loaded data) when the page closes, so:
// - reopening the same game shows its frame at once,
// - hovering a game card for a moment starts loading that video's index and first frame
//   (preload="metadata"), so it's usually ready by the time the click lands,
// - the browser doesn't build and tear down a media player on every page open.

import { fileSrc } from "./api";

let el: HTMLVideoElement | null = null;
let attached = false;
let warmTimer: ReturnType<typeof setTimeout> | undefined;

export function sharedVideo(): HTMLVideoElement {
  if (!el) {
    el = document.createElement("video");
    el.preload = "metadata";
    el.playsInline = true;
    el.disablePictureInPicture = true;
  }
  return el;
}

/** Video URL that changes when the file does (e.g. after it was finalized for playback). */
export function videoUrl(path: string | null | undefined, bytes?: number | null): string {
  const u = fileSrc(path);
  return u && bytes ? `${u}?v=${bytes}` : u;
}

function load(v: HTMLVideoElement, url: string) {
  if (v.dataset.src === url) return;
  v.dataset.src = url;
  v.src = url;
}

/** Start loading a game's video in the background (hovering its card). */
export function warmVideo(url: string) {
  if (attached || !url) return;
  clearTimeout(warmTimer);
  warmTimer = setTimeout(() => {
    if (attached) return;
    const v = sharedVideo();
    v.preload = "metadata";
    load(v, url);
  }, 120);
}

export function cancelWarm() {
  clearTimeout(warmTimer);
}

/** Puts the shared element into `host`, playing `url`. */
export function attachVideo(host: HTMLElement, url: string): HTMLVideoElement {
  clearTimeout(warmTimer);
  attached = true;
  const v = sharedVideo();
  v.preload = "auto";
  load(v, url);
  host.appendChild(v);
  return v;
}

export function setVideoUrl(url: string) {
  load(sharedVideo(), url);
}

export function detachVideo() {
  attached = false;
  const v = sharedVideo();
  v.pause();
  v.remove();
}
