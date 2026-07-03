import { copyFile, mkdir, cp, rm } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execSync } from 'node:child_process';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, '..');
const outDir = join(root, 'out');

async function pack() {
  console.log('Cleaning out directory...');
  await rm(outDir, { recursive: true, force: true });
  await mkdir(outDir, { recursive: true });

  console.log('Copying files...');
  await copyFile(join(root, 'package.json'), join(outDir, 'package.json'));
  await copyFile(join(root, 'server.js'), join(outDir, 'server.js'));
  await copyFile(join(root, 'db.js'), join(outDir, 'db.js'));
  await copyFile(join(root, 'senclaw-manifest.json'), join(outDir, 'senclaw-manifest.json'));
  
  console.log('Copying web/dist...');
  await mkdir(join(outDir, 'web'), { recursive: true });
  await cp(join(root, 'web', 'dist'), join(outDir, 'web', 'dist'), { recursive: true });

  console.log('Creating zip...');
  try {
    execSync('zip -r ../test-manager.zip .', { cwd: outDir, stdio: 'inherit' });
    console.log('Successfully created test-manager.zip');
  } catch (e) {
    console.error('Failed to create zip', e);
  }
}

pack().catch(console.error);
