import { defineConfig } from 'wxt';

export default defineConfig({
  outDir: 'dist',
  modules: ['@wxt-dev/module-react'],
  srcDir: 'src',
  manifest: {
    name: 'SenClaw Extension',
    description: 'Remote browser control for SemaClaw agents',
    version: '0.1.0',
    icons: {
      '16': 'icon.png',
      '32': 'icon.png',
      '48': 'icon.png',
      '128': 'icon.png',
    },
    permissions: [
      'tabs',
      'tabGroups',
      'activeTab',
      'storage',
      'scripting',
      'sidePanel',
    ],
    host_permissions: [
      '<all_urls>',
    ],
    side_panel: {
      default_path: 'sidepanel.html',
    },
  },
});
