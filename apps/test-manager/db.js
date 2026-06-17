import sqlite3 from 'sqlite3';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const dbPath = path.join(__dirname, 'test-manager.sqlite');
const db = new sqlite3.Database(dbPath);

export function initDb() {
  return new Promise((resolve, reject) => {
    db.serialize(() => {
      db.run(`
        CREATE TABLE IF NOT EXISTS requirements (
          id TEXT PRIMARY KEY,
          title TEXT,
          description TEXT,
          status TEXT DEFAULT 'Open'
        )
      `);
      db.run(`
        CREATE TABLE IF NOT EXISTS test_cases (
          id TEXT PRIMARY KEY,
          req_id TEXT,
          title TEXT,
          steps TEXT,
          expected_result TEXT,
          status TEXT DEFAULT 'Draft',
          last_run_log TEXT,
          FOREIGN KEY (req_id) REFERENCES requirements (id)
        )
      `, (err) => {
        if (err) reject(err);
        else resolve();
      });
    });
  });
}

export function runQuery(sql, params = []) {
  return new Promise((resolve, reject) => {
    db.run(sql, params, function (err) {
      if (err) reject(err);
      else resolve(this);
    });
  });
}

export function getQuery(sql, params = []) {
  return new Promise((resolve, reject) => {
    db.all(sql, params, (err, rows) => {
      if (err) reject(err);
      else resolve(rows);
    });
  });
}

export default db;
