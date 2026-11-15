export type FileClassification = 'NEW' | 'MISMATCH' | 'VERIFIED_IGNORED' | 'SKIPPED' | 'ERROR';

export interface DeviceInfo {
  id: string;
  name: string;
  manufacturer: string;
  description: string;
  is_iphone: boolean;
}

export interface ScannedFile {
  object_id: string;
  relative_path: string;
  filename: string;
  size: number;
  status: FileClassification;
  target_relative_path: string;
  existing_size?: number;
  error?: string;
}

export interface ScanSummary {
  total_iphone_files: number;
  total_backup_files: number;
  new_count: number;
  mismatch_count: number;
  verified_ignored_count: number;
  skipped_count: number;
  error_count?: number;
}

export interface ScanProgressPayload {
  stage: 'connecting' | 'enumerating' | 'indexing' | 'classifying' | 'completed' | 'cancelled';
  message: string;
  current: number;
  total: number;
  percent: number;
}

export interface ScanResult {
  files: ScannedFile[];
  summary: ScanSummary;
  skip_list: string[];
}

export interface SyncProgressPayload {
  current_file_index: number;
  total_files: number;
  filename: string;
  relative_path: string;
  bytes_copied: number;
  total_bytes: number;
  status: 'copying' | 'completed' | 'skipped' | 'error' | 'aborted' | 'finished';
  speed_bytes_sec: number;
  error?: string;
}
