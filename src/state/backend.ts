// Thin typed wrapper over the Rust IPC commands and events.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Listing } from "../tree/model";

export interface RootInfo {
  name: string;
  abs: string;
  initial: string | null;
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
}

export const backend = {
  getRoot: () => invoke<RootInfo>("get_root"),
  listDirectory: (path: string) => invoke<Listing>("list_directory", { path }),
  listDirectories: (paths: string[]) => invoke<Listing[]>("list_directories", { paths }),
  revealPath: (path: string) => invoke<Listing[]>("reveal_path", { path }),
  searchFiles: (query: string, limit = 40) => invoke<SearchResponse>("search_files", { query, limit }),
  indexStatus: () => invoke<IndexStatus>("index_status"),
  startIndexing: () => invoke<boolean>("start_indexing"),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  onFilesystemChanged: (cb: (dirs: string[]) => void): Promise<UnlistenFn> =>
    listen<{ dirs: string[] }>("filesystem_changed", (e) => cb(e.payload.dirs)),
  onIndex: (cb: (s: IndexStatus) => void): Promise<UnlistenFn[]> =>
    Promise.all([listen<IndexStatus>("index_progress", (e) => cb(e.payload)), listen<IndexStatus>("index_ready", (e) => cb(e.payload))]),
};

export type Backend = typeof backend;
