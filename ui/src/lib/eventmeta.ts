import type { EventKind } from "./types";

export type Group = "kills" | "deaths" | "assists" | "ult" | "structures" | "objectives" | "rounds" | "markers" | "game";

export interface KindMeta {
  label: string;
  color: string;
  icon: string; // key into ICONS
  group: Group;
}

export const KIND: Record<EventKind, KindMeta> = {
  kill: { label: "Kill", color: "var(--ev-kill)", icon: "crosshair", group: "kills" },
  multikill: { label: "Multikill", color: "var(--ev-kill)", icon: "burst", group: "kills" },
  first_blood: { label: "First blood", color: "var(--ev-kill)", icon: "drop", group: "kills" },
  ace: { label: "Ace", color: "var(--ev-kill)", icon: "crown", group: "kills" },
  death: { label: "Death", color: "var(--ev-death)", icon: "skull", group: "deaths" },
  assist: { label: "Assist", color: "var(--ev-assist)", icon: "assist", group: "assists" },
  ult_pressed: { label: "Ult pressed", color: "var(--ev-ult)", icon: "bolt", group: "ult" },
  tower: { label: "Tower", color: "var(--ev-structure)", icon: "tower", group: "structures" },
  inhibitor: { label: "Inhibitor", color: "var(--ev-structure)", icon: "gem", group: "structures" },
  dragon: { label: "Dragon", color: "var(--ev-epic)", icon: "flame", group: "objectives" },
  herald: { label: "Herald", color: "var(--ev-epic)", icon: "eye", group: "objectives" },
  baron: { label: "Baron", color: "var(--ev-epic)", icon: "horns", group: "objectives" },
  objective: { label: "Objective", color: "var(--ev-epic)", icon: "hex", group: "objectives" },
  round: { label: "Round", color: "var(--ev-neutral)", icon: "flag", group: "rounds" },
  manual_marker: { label: "Marker", color: "var(--ev-marker)", icon: "bookmark", group: "markers" },
  clip: { label: "Clip", color: "var(--ev-clip)", icon: "film", group: "markers" },
  game_start: { label: "Game start", color: "var(--ev-neutral)", icon: "play", group: "game" },
  game_end: { label: "Game end", color: "var(--ev-neutral)", icon: "flag", group: "game" },
};

export const GROUPS: { id: Group; label: string; color: string; icon: string }[] = [
  { id: "kills", label: "Kills", color: "var(--ev-kill)", icon: "crosshair" },
  { id: "deaths", label: "Deaths", color: "var(--ev-death)", icon: "skull" },
  { id: "assists", label: "Assists", color: "var(--ev-assist)", icon: "assist" },
  { id: "ult", label: "Ult", color: "var(--ev-ult)", icon: "bolt" },
  { id: "structures", label: "Towers", color: "var(--ev-structure)", icon: "tower" },
  { id: "objectives", label: "Objectives", color: "var(--ev-epic)", icon: "flame" },
  { id: "rounds", label: "Rounds", color: "var(--ev-neutral)", icon: "flag" },
  { id: "markers", label: "Markers & clips", color: "var(--ev-marker)", icon: "bookmark" },
  { id: "game", label: "Game", color: "var(--ev-neutral)", icon: "flag" },
];

/** Kinds offered for auto-clips / callouts in Settings. */
export const USER_KINDS: EventKind[] = [
  "kill",
  "multikill",
  "first_blood",
  "ace",
  "death",
  "assist",
  "ult_pressed",
  "tower",
  "inhibitor",
  "dragon",
  "herald",
  "baron",
  "objective",
  "manual_marker",
  "clip",
];

// 24x24 stroke icons (own artwork).
export const ICONS: Record<string, string> = {
  crosshair: "M12 3v4M12 17v4M3 12h4M17 12h4M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z",
  burst: "M12 2l2.2 5.6L20 6l-3.3 5 4.3 4-5.8.4L14 21l-2-5.2L10 21l-1.2-5.6L3 15l4.3-4L4 6l5.8 1.6z",
  drop: "M12 3c3 4.2 6 7.4 6 10.5A6 6 0 0 1 6 13.5C6 10.4 9 7.2 12 3z",
  crown: "M4 18h16M4 18l-1-10 5 4 4-7 4 7 5-4-1 10",
  skull: "M12 3a8 8 0 0 0-8 8c0 2.6 1.3 4.5 3 5.6V20h10v-3.4c1.7-1.1 3-3 3-5.6a8 8 0 0 0-8-8zM9 11.5h.01M15 11.5h.01M10 20v-2M14 20v-2",
  assist: "M8 12l3 3 5-6M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z",
  bolt: "M13 2L4 14h7l-1 8 9-12h-7z",
  tower: "M7 21h10M8 21l1-11h6l1 11M7 10h10M8 10V5l2 1 2-2 2 2 2-1v5",
  gem: "M6 3h12l4 6-10 12L2 9zM2 9h20M12 21L8 9l4-6 4 6z",
  flame: "M12 22c4 0 7-2.7 7-6.8 0-3.7-2.6-6-4.4-8.2-.5 2.3-1.8 3.5-2.8 3.5.4-3.2-.6-6.2-3.3-8.5.1 3.9-3.5 6.4-3.5 11.2C5 19.3 8 22 12 22z",
  eye: "M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12zM12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
  horns: "M5 3c0 5 2 8 7 8s7-3 7-8M7 13l-2 8h14l-2-8M9.5 16h.01M14.5 16h.01",
  hex: "M12 2l8.7 5v10L12 22l-8.7-5V7z",
  bookmark: "M6 3h12v18l-6-4-6 4z",
  film: "M4 4h16v16H4zM8 4v16M16 4v16M4 9h4M4 15h4M16 9h4M16 15h4",
  play: "M7 4l13 8-13 8z",
  flag: "M5 21V4M5 4h12l-2 4 2 4H5",
  // UI icons
  home: "M3 11l9-8 9 8M5 9.5V21h5v-6h4v6h5V9.5",
  library: "M4 4h6v16H4zM14 4l6 1.5-3.8 15L12.4 19z",
  clips: "M3 7h18v12H3zM3 7l3-4h12l3 4M10 10.5v5l4-2.5z",
  settings: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
  star: "M12 3l2.8 5.7 6.2.9-4.5 4.4 1 6.2L12 17.3 6.5 20.2l1-6.2L3 9.6l6.2-.9z",
  trash: "M4 7h16M10 11v6M14 11v6M5 7l1 13h12l1-13M9 7V4h6v3",
  folder: "M3 6a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z",
  back: "M15 5l-7 7 7 7",
  prev: "M18 6l-8 6 8 6zM6 6v12",
  next: "M6 6l8 6-8 6zM18 6v12",
  pause: "M7 5h3v14H7zM14 5h3v14h-3z",
  volume: "M4 9h4l5-4v14l-5-4H4zM16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12",
  mute: "M4 9h4l5-4v14l-5-4H4zM17 9l5 6M22 9l-5 6",
  fullscreen: "M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5",
  scissors: "M6 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM20 4L8.1 15.9M14.5 14.5L20 20M8.1 8.1L12 12",
  check: "M4 12l5 5L20 6",
  x: "M6 6l12 12M18 6L6 18",
  rec: "M12 12m-6 0a6 6 0 1 0 12 0 6 6 0 1 0-12 0",
  gamepad: "M6 11h4M8 9v4M15 12h.01M18 10h.01M17.3 5H6.7a4 4 0 0 0-3.9 3.2l-1.4 7A3 3 0 0 0 4.3 19c1 0 1.9-.5 2.4-1.3L8 16h8l1.3 1.7c.5.8 1.4 1.3 2.4 1.3a3 3 0 0 0 2.9-3.8l-1.4-7A4 4 0 0 0 17.3 5z",
  cpu: "M6 6h12v12H6zM9 9h6v6H9zM9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4",
  download: "M12 3v12M7 10l5 5 5-5M4 21h16",
  external: "M14 4h6v6M20 4l-9 9M18 14v6H4V6h6",
  info: "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20zM12 16v-5M12 8h.01",
  warn: "M12 3l10 18H2zM12 10v5M12 18h.01",
  keyboard: "M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10",
  search: "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM21 21l-5-5",
};
