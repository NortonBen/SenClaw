export interface Host {
  id: string;
  name: string;
  host: string;
  port: number;
  user: string;
  password?: string;
  keychain_id?: string;
  tags: string[];
}

export type TabType = 'home' | 'terminal' | 'sftp';

export interface AppTab {
  id: string;
  type: TabType;
  title: string;
  host?: Host;
}

export type KeychainItemType = 'Password' | 'PrivateKey';

export interface KeychainItem {
  id: string;
  name: string;
  item_type: KeychainItemType;
  value: String;
}

export interface PortForwardingRule {
  id: string;
  name: string;
  host_id: string;
  local_port: number;
  bind_address: string;
  destination_address: string;
  destination_port: number;
  active: boolean;
}

export interface FileNode {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified_time: number;
}
