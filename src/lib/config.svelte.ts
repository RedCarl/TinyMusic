/** 全局生效的应用配置（目前主要是「网盘是否启用」） */
import { api } from './api';

export const appCfg = $state({
  loaded: false,
  s3Enabled: false,
});

export async function refreshAppCfg() {
  try {
    const s = await api.settingsGet();
    appCfg.s3Enabled = !!s.s3_enabled && !!s.s3_bucket;
    appCfg.loaded = true;
    return s;
  } catch {
    appCfg.s3Enabled = false;
    appCfg.loaded = true;
    return null;
  }
}
