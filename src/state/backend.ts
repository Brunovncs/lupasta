// Thin typed wrapper over the Rust IPC commands and events.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Listing } from "../tree/model";

export interface RootInfo {
  name: string;
  abs: string;
  parent?: string | null;
  initial: string | null;
  generation?: number;
}

export interface SearchHit {
  path: string;
  name: string;
  kind: string;
  dir: boolean;
  score: number;
}

export interface SearchResponse {
  query: string;
  hits: SearchHit[];
  candidates: number;
  strategy: string;
  micros: number;
}

export interface IndexStatus {
  state: "idle" | "loading" | "indexing" | "ready";
  indexed: number;
  corpus: number;
  last_scan_ms: number;
  enabled?: boolean;
  watcher_error?: string | null;
}

/** Mirrors `settings::Settings` on the Rust side. */
export interface Settings {
  color_mode: "age" | "kind";
  age_max_days: number;
  font: string;
  zoom: number;
  animation_ms: number;
  max_chars: number;
  status_line: boolean;
  show_hidden: boolean;
  sort: "name" | "modified" | "size";
  folders_first: boolean;
  wheel_step: number;
  restore_last: boolean;
  last: { root: string; select: string | null } | null;
  recent_roots: string[];
  respect_gitignore: boolean;
  excludes: string[];
}

export interface AppInfo {
  version: string;
  os: string;
  data_dir: string;
  capture: boolean;
  smoke: boolean;
}

export interface UpdateInfo {
  current: string;
  latest: string | null;
  newer: boolean;
  url: string | null;
}

export const backend = {
  getRoot: () => invoke<RootInfo>("get_root"),
  listDirectory: (path: string) => invoke<Listing>("list_directory", { path }),
  listDirectories: (paths: string[]) => invoke<Listing[]>("list_directories", { paths }),
  revealPath: (path: string) => invoke<Listing[]>("reveal_path", { path }),
  searchFiles: (query: string, limit = 40) => invoke<SearchResponse>("search_files", { query, limit }),
  indexStatus: () => invoke<IndexStatus>("index_status"),
  startIndexing: () => invoke<boolean>("start_indexing"),
  clearIndex: () => invoke<boolean>("clear_index"),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  revealInOs: (path: string) => invoke<void>("reveal_in_os", { path }),
  absPath: (path: string) => invoke<string>("abs_path", { path }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<Settings>("set_settings", { settings }),
  rememberSelection: (path: string) => invoke<void>("remember_selection", { path }),
  setRoot: (path: string, select: string | null = null) => invoke<RootInfo>("set_root", { path, select }),
  appInfo: () => invoke<AppInfo>("app_info"),
  openDataDir: () => invoke<void>("open_data_dir"),
  checkUpdate: () => invoke<UpdateInfo>("check_update"),
  openRelease: (url: string) => invoke<void>("open_release", { url }),
  smokeReport: (ok: boolean, detail: string) => invoke<void>("smoke_report", { ok, detail }),
  onFilesystemChanged: (cb: (dirs: string[]) => void): Promise<UnlistenFn> =>
    listen<{ dirs: string[] }>("filesystem_changed", (e) => cb(e.payload.dirs)),
  onIndex: (cb: (s: IndexStatus) => void): Promise<UnlistenFn[]> =>
    Promise.all([listen<IndexStatus>("index_progress", (e) => cb(e.payload)), listen<IndexStatus>("index_ready", (e) => cb(e.payload))]),
};

export type Backend = typeof backend;

/** The part of the backend the navigation controller uses (and unit tests fake). */
export type NavBackend = Pick<
  Backend,
  "getRoot" | "listDirectory" | "listDirectories" | "revealPath" | "searchFiles" | "indexStatus" | "startIndexing" | "openPath" | "onFilesystemChanged" | "onIndex"
>;
