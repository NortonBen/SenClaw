package relay

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"sync"
	"time"

	"github.com/gorilla/websocket"
	"semaclaw/hub-backend/internal/store"
)

type clientConn struct {
	send    func(*RelayFrame) error
	isAlive func() bool
}

type Server struct {
	mu sync.RWMutex
	// channel_id -> sender_id -> websocket
	channels map[string]map[string]*clientConn
	db       *store.Store
}

type RelayFrame struct {
	Type        string `json:"type"`
	ChannelID   string `json:"channel_id"`
	SenderID    string `json:"sender_id"`
	Timestamp   int64  `json:"timestamp"`
	MessageID   string `json:"message_id"`
	ControlType *int   `json:"control_type,omitempty"`
	Metadata    string `json:"metadata,omitempty"`
	Nonce       string `json:"nonce,omitempty"`
	Ciphertext  string `json:"ciphertext,omitempty"`
	Tag         string `json:"tag,omitempty"`
}

func NewServer(db *store.Store) *Server {
	return &Server{
		channels: make(map[string]map[string]*clientConn),
		db:       db,
	}
}

// StartHeartbeat runs a background loop to send PING messages to all connected clients
func (s *Server) StartHeartbeat(interval time.Duration) {
	ticker := time.NewTicker(interval)
	defer ticker.Stop()

	for {
		<-ticker.C

		type clientInfo struct {
			channelID string
			senderID  string
			conn      *clientConn
		}
		var activeClients []clientInfo

		s.mu.RLock()
		for channelID, clients := range s.channels {
			for senderID, conn := range clients {
				activeClients = append(activeClients, clientInfo{
					channelID: channelID,
					senderID:  senderID,
					conn:      conn,
				})
			}
		}
		s.mu.RUnlock()

		for _, info := range activeClients {
			pingMsg := &RelayFrame{
				Type:      "ping",
				ChannelID: info.channelID,
				SenderID:  "server",
				Timestamp: time.Now().UnixMilli(),
				MessageID: fmt.Sprintf("ping-%d", time.Now().UnixMilli()),
			}
			if !info.conn.isAlive() {
				s.removeClient(info.channelID, info.senderID)
				continue
			}
			if err := info.conn.send(pingMsg); err != nil {
				log.Printf("Heartbeat failed for Channel=%s, Sender=%s: %v", info.channelID, info.senderID, err)
				s.removeClient(info.channelID, info.senderID)
			}
		}
	}
}

func (s *Server) addClient(channelID, senderID string, conn *clientConn) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	ch, ok := s.channels[channelID]
	if !ok {
		ch = make(map[string]*clientConn)
		s.channels[channelID] = ch
	}

	// Clean up dead connections in this channel
	for id, oldConn := range ch {
		if !oldConn.isAlive() {
			delete(ch, id)
			log.Printf("Cleaned up stale connection: Channel=%s, Sender=%s", channelID, id)
		}
	}

	// Limit to max 2 connections (Senclaw and App Connector)
	// If the sender is reconnecting, allow replacing the old one
	if len(ch) >= 2 && ch[senderID] == nil {
		return fmt.Errorf("channel is full, maximum 2 connections allowed")
	}

	ch[senderID] = conn
	return nil
}

func (s *Server) removeClient(channelID, senderID string) {
	if channelID == "" || senderID == "" {
		return
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	if ch, ok := s.channels[channelID]; ok {
		delete(ch, senderID)
		if len(ch) == 0 {
			delete(s.channels, channelID)
		}
		log.Printf("Client disconnected. Channel: %s, Sender: %s", channelID, senderID)
	}
}

func (s *Server) broadcast(channelID, senderID string, msg *RelayFrame) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	ch, ok := s.channels[channelID]
	if !ok {
		return
	}

	for id, stream := range ch {
		if id == senderID {
			log.Printf("Skipping message to self: %s", senderID)
			continue // Do not echo back to sender
		}
		log.Printf("Broadcasting message %s to %s in channel %s", msg.MessageID, id, channelID)
		if !stream.isAlive() {
			log.Printf("Dropping stale client %s in channel %s", id, channelID)
			continue
		}
		if err := stream.send(msg); err != nil {
			log.Printf("Failed to send message to %s in channel %s: %v", id, channelID, err)
		}
	}
}

var upgrader = websocket.Upgrader{
	CheckOrigin: func(r *http.Request) bool { return true },
}

const (
	wsPongWait   = 75 * time.Second
	wsPingPeriod = 25 * time.Second
	wsWriteWait  = 10 * time.Second
)

// HandleWebSocket exposes the relay stream over WebSocket.
// Query params: channel_id, access_token.
// Payload: JSON frames.
func (s *Server) HandleWebSocket(w http.ResponseWriter, r *http.Request) {
	channelID := r.URL.Query().Get("channel_id")
	token := r.URL.Query().Get("access_token")
	if channelID == "" || token == "" {
		http.Error(w, "missing authentication credentials", http.StatusBadRequest)
		return
	}
	if !s.db.VerifyChannel(channelID, token) {
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}

	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		log.Printf("ws upgrade failed: %v", err)
		return
	}
	defer conn.Close()

	var (
		senderID string
		writeMu  sync.Mutex
		closed   atomicBool
	)
	conn.SetReadLimit(8 << 20)
	_ = conn.SetReadDeadline(time.Now().Add(wsPongWait))
	conn.SetPongHandler(func(_ string) error {
		return conn.SetReadDeadline(time.Now().Add(wsPongWait))
	})

	sendFn := func(msg *RelayFrame) error {
		if closed.Load() {
			return fmt.Errorf("connection closed")
		}
		data, err := json.Marshal(msg)
		if err != nil {
			return err
		}
		writeMu.Lock()
		defer writeMu.Unlock()
		return conn.WriteMessage(websocket.TextMessage, data)
	}
	go func() {
		ticker := time.NewTicker(wsPingPeriod)
		defer ticker.Stop()
		for range ticker.C {
			if closed.Load() {
				return
			}
			writeMu.Lock()
			_ = conn.SetWriteDeadline(time.Now().Add(wsWriteWait))
			err := conn.WriteControl(websocket.PingMessage, []byte("keepalive"), time.Now().Add(wsWriteWait))
			writeMu.Unlock()
			if err != nil {
				closed.Store(true)
				s.removeClient(channelID, senderID)
				_ = conn.Close()
				return
			}
		}
	}()

	for {
		msgType, raw, err := conn.ReadMessage()
		if err != nil {
			closed.Store(true)
			s.removeClient(channelID, senderID)
			return
		}
		if msgType != websocket.TextMessage {
			continue
		}
		msg := &RelayFrame{}
		if err := json.Unmarshal(raw, msg); err != nil {
			log.Printf("ws decode error: %v", err)
			continue
		}

		if senderID == "" {
			if msg.ChannelID != "" && msg.ChannelID != channelID {
				log.Printf("channel_id mismatch over ws: got=%s expected=%s", msg.ChannelID, channelID)
				continue
			}
			senderID = msg.SenderID
			if senderID == "" {
				log.Printf("ws sender_id empty in first frame")
				continue
			}
			client := &clientConn{
				send: sendFn,
				isAlive: func() bool {
					return !closed.Load()
				},
			}
			if err := s.addClient(channelID, senderID, client); err != nil {
				log.Printf("ws add client failed: %v", err)
				return
			}
			log.Printf("WS client connected. Channel: %s, Sender: %s", channelID, senderID)
		}

		s.broadcast(channelID, senderID, msg)
	}
}

type atomicBool struct {
	mu sync.RWMutex
	v  bool
}

func (b *atomicBool) Load() bool {
	b.mu.RLock()
	defer b.mu.RUnlock()
	return b.v
}

func (b *atomicBool) Store(v bool) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.v = v
}
