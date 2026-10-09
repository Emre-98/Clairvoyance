// Mirrors of the Rust types (serde JSON).

export type EventKind =
  | "kill"
  | "death"
  | "assist"
  | "multikill"
  | "first_blood"
  | "ace"
  | "ult_pressed"
  | "ult_used"
  | "ult_unconfirmed"
  | "ult_recast"
  | "form_swap"
  | "tower"
  | "inhibitor"
  | "dragon"
  | "herald"
  | "baron"
  | "objective"
  | "item_completed"
  | "summoner_spell"
  | "round"
  | "manual_marker"
  | "clip"
  | "game_start"
  | "game_end";

export interface GameEvent {
  id: string;
  kind: EventKind;
  game_time: number;
  title: string;
  details?: string;
  steal?: boolean;
  /** Picture for the hover card (League: item / summoner spell from Data Dragon). */
  icon?: EventIcon | null;
  /** Labelled details, e.g. ["Lane", "Mid"]. */
  facts?: [string, string][];
  /** Characters involved (League: champion ids), the main one first. */
  who?: string[];
}

export interface EventIcon {
  kind: "item" | "spell" | "champion" | string;
  id: string;
  /** Data version (League: Data Dragon "16.20.1"); missing = the newest. */
  version?: string;
}

export interface PlayerInfo {
  name: string;
  character?: string | null;
  character_id?: string | null;
  team?: string | null;
  mode?: string | null;
}

export interface PlayerStats {
  kills: number;
  deaths: number;
  assists: number;
  cs?: number | null;
  gold?: number | null;
  level?: number | null;
  vision_score?: number | null;
  extra: [string, string][];
}

export type GameResult = "win" | "loss" | "draw";

export interface RecorderStatus {
  connected: boolean;
  recording: boolean;
  replay_buffer: boolean;
  version?: string | null;
  encoder?: string | null;
  hardware_encoder: boolean;
  error?: string | null;
}

export interface LiveStatus {
  state: "idle" | "detected" | "recording";
  game_id?: string | null;
  game_name?: string | null;
  phase: "waiting" | "loading" | "in_progress" | "ended";
  game_time?: number | null;
  session_id?: string | null;
  player?: PlayerInfo | null;
  stats?: PlayerStats | null;
  event_count: number;
  last_event?: GameEvent | null;
  video_offset?: number | null;
  recorder: RecorderStatus;
  message?: string | null;
  mode_name?: string | null;
  mode_rule?: ModeRule | null;
  /** A replay / spectating: not recorded (v1.7.1). */
  watching?: WatchKind | null;
}

export type WatchKind = "replay" | "spectate" | "unknown";

export type EngineEvent =
  | ({ type: "status" } & LiveStatus)
  | { type: "game_event"; session_id: string; event: GameEvent }
  | { type: "game_started"; game_name: string }
  | { type: "game_ended"; session_id: string }
  | { type: "library_changed" }
  | { type: "notice"; level: string; text: string };

export interface SessionSummary {
  id: string;
  dir: string;
  game_id: string;
  game_name: string;
  started_at: string;
  player?: PlayerInfo | null;
  stats?: PlayerStats | null;
  result?: GameResult | null;
  video_path?: string | null;
  thumb_path?: string | null;
  duration?: number | null;
  favorite: boolean;
  /** Clips marked "keep" (never auto-deleted). */
  kept_clips: number;
  /** The storage clean-up removed the full video (clips kept). */
  video_removed: boolean;
  size_bytes: number;
  video_bytes: number;
  /** Size of the input recording (replay overlay); 0 = none. */
  input_bytes?: number;
  event_count: number;
  clip_count: number;
  thumb_at: number;
  queue_id?: number | null;
  mode_name?: string | null;
  mode_key?: string | null;
  record_mode?: "full" | "clips_only" | null;
}

export interface ClipInfo {
  file: string;
  title: string;
  video_start?: number | null;
  video_end?: number | null;
  created_at: string;
  source: string;
  keep?: boolean;
}

export interface StatSample {
  t: number;
  kills: number;
  deaths: number;
  assists: number;
  cs?: number | null;
  gold?: number | null;
}

export interface PerfStats {
  samples: number;
  cpu_avg: number;
  cpu_max: number;
  ram_avg_mb: number;
  ram_max_mb: number;
}

export interface GameSession {
  version: number;
  id: string;
  game_id: string;
  game_name: string;
  started_at: string;
  ended_at?: string | null;
  video_file?: string | null;
  video_offset: number;
  video_duration?: number | null;
  game_duration?: number | null;
  player?: PlayerInfo | null;
  stats?: PlayerStats | null;
  result?: GameResult | null;
  events: GameEvent[];
  clips: ClipInfo[];
  timeline: StatSample[];
  perf: PerfStats;
  favorite: boolean;
  video_removed_at?: string | null;
  queue_id?: number | null;
  mode_name?: string | null;
  mode_key?: string | null;
  record_mode?: "full" | "clips_only" | null;
  warnings: string[];
  verification?: Verification | null;
  input_file?: string | null;
  mechanics?: Mechanics | null;
  /** Everyone's numbers over the game, changes only (v1.8+). */
  scoreboard?: Scoreboard | null;
}

/** cv_core::scoreboard (time-synced scoreboard). */
export interface SbPlayer {
  name: string;
  character: string;
  character_id: string;
  team: string;
  me?: boolean;
}
export interface PlayerState {
  level: number;
  kills: number;
  deaths: number;
  assists: number;
  cs: number;
  /** Item ids by slot (0 = empty; 6 slots + trinket). */
  items: number[];
  /** Summoner spell ids, D then F. */
  spells: string[];
}
export interface SbDelta {
  i: number;
  lv?: number;
  k?: number;
  d?: number;
  a?: number;
  cs?: number;
  it?: number[];
  sp?: string[];
}
export interface Scoreboard {
  version: string;
  players: SbPlayer[];
  names: Record<string, string>;
  frames: { t: number; d: SbDelta[] }[];
}

/** Live key presses checked against the recording after the game (League: ult casts). */
export interface Verification {
  version: number;
  status: "verified" | "skipped" | "failed";
  reason?: string | null;
  confidence: number;
  casts: number;
  confirmed: number;
  video_only: number;
  unconfirmed: number;
  analysis_ms: number;
}

/** Mouse/keyboard stats of a game or a range (cv_core::input::stats::Mechanics). */
export interface Mechanics {
  version: number;
  from: number;
  to: number;
  focused_secs: number;
  clicks: number;
  right_clicks: number;
  key_presses: number;
  apm: number;
  apm_per_min: (number | null)[];
  /** APM per 10 s of video time from 0 (whole game only; stats version 2+). */
  apm_bins?: (number | null)[];
  right_click_hz: number;
  cursor_distance: number;
  path_efficiency?: number | null;
  idle_secs: number;
  cursor_samples: number;
}

export interface SessionView {
  /** Size of the input recording (replay overlay), if any. */
  input_bytes?: number | null;
  session: GameSession;
  dir: string;
  video_path?: string | null;
  video_bytes?: number | null;
  thumb_path?: string | null;
  clips: (ClipInfo & { path: string; exists: boolean; thumb_path?: string | null })[];
}

export interface ClipEntry {
  session_id: string;
  game_name: string;
  character?: string | null;
  character_id?: string | null;
  path: string;
  file: string;
  title: string;
  created_at: string;
  size_bytes: number;
  source: string;
  keep: boolean;
  thumb_path?: string | null;
  duration?: number | null;
}

export interface VideoSettings {
  encoder: string;
  quality: string;
  fps: number;
  height: number;
  replay_buffer_secs: number;
  record_mic: boolean;
  display_capture: boolean;
  /** "h264" | "hevc" | "av1" (v1.8). */
  codec?: string;
  /** "bitrate" | "quality" (v1.8). */
  rate_control?: string;
  /** Codecs the in-app player can play (reported by the UI). */
  playable_codecs?: string[];
}

export interface EventSettings {
  clip_kinds: EventKind[];
  clip_before_secs: number;
  clip_after_secs: number;
  tts_enabled: boolean;
  tts_kinds: EventKind[];
  tts_volume: number;
}

export interface Settings {
  first_run_done: boolean;
  save_dir: string;
  auto_delete_days: number;
  max_disk_gb: number;
  auto_cleanup: boolean;
  hotkey_clip: string;
  hotkey_marker: string;
  auto_record: boolean;
  close_ui_in_game: boolean;
  keep_ui_loaded: boolean;
  start_with_windows: boolean;
  start_minimized: boolean;
  show_perf: boolean;
  dev_tools: boolean;
  theme: "system" | "dark" | "light";
  auto_update_check: boolean;
  video: VideoSettings;
  events: EventSettings;
  games: Record<string, Record<string, unknown>>;
  disabled_games: string[];
  ffmpeg_path: string;
  /** Share makes a copy under Discord's free upload limit (on by default). */
  share_fit_discord: boolean;
}

/** What Share put on the clipboard. */
export interface Shared {
  path: string;
  bytes: number;
  width: number;
  height: number;
  fps: number;
  /** A Discord copy (made or reused), not the clip itself. */
  fitted: boolean;
  /** On the clipboard (else only `path` is usable). */
  copied: boolean;
}

export interface ConfigField {
  key: string;
  label: string;
  kind: "text" | "key" | "bool" | "select";
  help: string;
  options?: [string, string][];
}

export interface GameMeta {
  id: string;
  name: string;
  short_name: string;
  supports_events: boolean;
  input_tracking?: boolean;
  /** Sub-toggles of the replay overlay's "Ability bubbles" (empty: none for this game). */
  action_categories?: { id: string; label: string }[];
  config_fields: ConfigField[];
  default_config: Record<string, unknown>;
}

export interface GpuInfo {
  vendor: string;
  name: string;
  encoder: string;
}

export interface AppInfo {
  version: string;
  log_file: string;
  config_file: string;
  default_save_dir: string;
  save_dir: string;
  gpu?: GpuInfo | null;
  games: GameMeta[];
  /** The old app (GameRecorder, before the rename) is still installed. */
  legacy_install: boolean;
}

export interface CleanupDone {
  at: string;
  id: string;
  title: string;
  action: "delete_game" | "delete_video";
  bytes: number;
  reason: string;
}

export interface StorageInfo {
  save_dir: string;
  used_bytes: number;
  limit_bytes: number;
  auto_cleanup: boolean;
  free_bytes?: number | null;
  games: number;
  protected_bytes: number;
  stuck_over_limit: boolean;
  recent_cleanups: CleanupDone[];
  thumbnails_dir: string;
}

export interface CleanupReport {
  thumbs_made: number;
  removed: CleanupDone[];
  freed_bytes: number;
  still_over: boolean;
  skipped_busy: boolean;
}

export interface PhaseResult {
  name: string;
  secs: number;
  frames: number;
  fps_avg?: number | null;
  fps_1_low?: number | null;
  frametime_p99_ms?: number | null;
  game_cpu: number;
  total_cpu: number;
  app_cpu: number;
  app_ram_mb: number;
  gpu_3d: number;
  gpu_encode: number;
  game_gpu_3d: number;
}

export interface PerfReport {
  started_at: string;
  gpu: string;
  encoder: string;
  fps_source: string;
  phases: PhaseResult[];
  notes: string[];
}

export interface PerfTestStatus {
  state: "" | "idle" | "preparing" | "waiting_for_game" | "running" | "waiting_for_exit" | "analyzing" | "done" | "error";
  phase?: string | null;
  phase_ends_in?: number | null;
  message?: string | null;
  report?: PerfReport | null;
  report_text?: string | null;
  report_path?: string | null;
}

export type ModeRule = "record" | "clips_only" | "off";

export interface ModeEntry {
  name: string;
  queue_id?: number | null;
  game_mode?: string | null;
  group: string;
  rule: ModeRule;
  /** false = not currently available (kept so the choice returns with the mode). */
  available?: boolean | null;
  is_new: boolean;
  first_seen?: string | null;
}

export interface GameModes {
  unknown_rule: ModeRule;
  entries: Record<string, ModeEntry>;
  catalog_updated_at?: string | null;
}

export interface ModeGroupInfo {
  id: string;
  label: string;
  help: string;
}

export interface GameModesView {
  game_id: string;
  game_name: string;
  groups: ModeGroupInfo[];
  modes: GameModes;
}

export interface UpdateStatus {
  state: "idle" | "checking" | "up_to_date" | "available" | "downloading" | "installing" | "error";
  current_version: string;
  version?: string | null;
  notes?: string | null;
  date?: string | null;
  downloaded: number;
  total?: number | null;
  error?: string | null;
  checked_at?: string | null;
}

export interface SelfTestResult {
  path: string;
  bytes: number;
  frames: number;
  dropped: number;
  fps: number;
  cpu_percent: number;
}
