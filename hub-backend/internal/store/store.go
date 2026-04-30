package store

import (
	"crypto/rand"
	"database/sql"
	"encoding/hex"
	"log"
	"time"

	_ "github.com/mattn/go-sqlite3"
)

type Store struct {
	db *sql.DB
}

func NewStore(dbPath string) (*Store, error) {
	db, err := sql.Open("sqlite3", dbPath)
	if err != nil {
		return nil, err
	}

	if err := db.Ping(); err != nil {
		return nil, err
	}

	s := &Store{db: db}
	if err := s.initSchema(); err != nil {
		return nil, err
	}

	return s, nil
}

func (s *Store) initSchema() error {
	query := `
	CREATE TABLE IF NOT EXISTS agents (
		id TEXT PRIMARY KEY,
		name TEXT,
		avatar_url TEXT,
		status TEXT
	);

	CREATE TABLE IF NOT EXISTS messages (
		id TEXT PRIMARY KEY,
		agent_id TEXT,
		role TEXT,
		content TEXT,
		timestamp TEXT,
		is_encrypted BOOLEAN
	);

	CREATE TABLE IF NOT EXISTS skills (
		slug TEXT PRIMARY KEY,
		display_name TEXT,
		summary TEXT,
		created_at INTEGER,
		updated_at INTEGER
	);

	CREATE TABLE IF NOT EXISTS channels (
		id TEXT PRIMARY KEY,
		access_token TEXT NOT NULL,
		created_at INTEGER
	);
	`
	_, err := s.db.Exec(query)
	if err != nil {
		log.Printf("Failed to initialize schema: %v", err)
	}
	return err
}

func (s *Store) Close() error {
	return s.db.Close()
}

func generateToken(length int) string {
	b := make([]byte, length)
	if _, err := rand.Read(b); err != nil {
		return "fallback-token-xyz"
	}
	return hex.EncodeToString(b)
}

func (s *Store) RegisterChannel() (string, string, error) {
	channelID := "ch_" + generateToken(8)
	token := generateToken(16)
	createdAt := time.Now().Unix()

	query := `INSERT INTO channels (id, access_token, created_at) VALUES (?, ?, ?)`
	_, err := s.db.Exec(query, channelID, token, createdAt)
	if err != nil {
		return "", "", err
	}
	return channelID, token, nil
}

func (s *Store) VerifyChannel(channelID, token string) bool {
	var dbToken string
	err := s.db.QueryRow(`SELECT access_token FROM channels WHERE id = ?`, channelID).Scan(&dbToken)
	if err != nil {
		return false
	}
	return dbToken == token
}
