// Mirrors of the Rust types (serde JSON).

export type EventKind =
  | "kill"
  | "death"
  | "assist"
  | "multikill"
  | "first_blood"
  | "ace"
  | "ult_pressed"
  | "tower"
  | "inhibitor"
  | "dragon"
  | "herald"
  | "baron"
  | "objective"
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
}

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
  event_count: number;
  clip_count: number;
  thumb_at: number;
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
  warnings: string[];
}

export interface SessionView {
  session: GameSession;
  dir: string;
  video_path?: string | null;
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
  theme: "system" | "dark" | "light";
  auto_update_check: boolean;
  video: VideoSettings;
  events: EventSettings;
  games: Record<string, Record<string, unknown>>;
  disabled_games: string[];
  ffmpeg_path: string;
}

export interface ConfigField {
  key: string;
  label: string;
  kind: "text" | "key" | "bool";
  help: string;
}

export interface GameMeta {
  id: string;
  name: string;
  short_name: string;
  supports_events: boolean;
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
