package relay

import (
	"fmt"
	"io"
	"log"
	"sync"

	"google.golang.org/grpc/metadata"

	"semaclaw/hub-backend/internal/store"
	pb "semaclaw/hub-backend/pkg/proto"
)

// Server implements the ChannelRelay server
type Server struct {
	pb.UnimplementedChannelRelayServer
	
	mu       sync.RWMutex
	// channel_id -> sender_id -> stream
	channels map[string]map[string]pb.ChannelRelay_StreamServer
	db       *store.Store
}

func NewServer(db *store.Store) *Server {
	return &Server{
		channels: make(map[string]map[string]pb.ChannelRelay_StreamServer),
		db:       db,
	}
}

func (s *Server) Stream(stream pb.ChannelRelay_StreamServer) error {
	// Extract metadata
	md, ok := metadata.FromIncomingContext(stream.Context())
	if !ok {
		return fmt.Errorf("missing metadata")
	}

	channelIDs := md.Get("channel_id")
	tokens := md.Get("access_token")

	if len(channelIDs) == 0 || len(tokens) == 0 {
		return fmt.Errorf("missing authentication credentials")
	}

	channelID := channelIDs[0]
	token := tokens[0]

	// Verify against db
	if !s.db.VerifyChannel(channelID, token) {
		return fmt.Errorf("unauthorized")
	}

	var senderID string

	for {
		msg, err := stream.Recv()
		if err == io.EOF {
			s.removeClient(channelID, senderID)
			return nil
		}
		if err != nil {
			log.Printf("Error receiving from stream: %v", err)
			s.removeClient(channelID, senderID)
			return err
		}

		if senderID == "" {
			if msg.ChannelId != "" && msg.ChannelId != channelID {
				return fmt.Errorf("channel_id mismatch")
			}
			senderID = msg.SenderId
			if err := s.addClient(channelID, senderID, stream); err != nil {
				return err
			}
			log.Printf("Client connected. Channel: %s, Sender: %s", channelID, senderID)
		}

		// Broadcast to all other participants in the same channel
		s.broadcast(channelID, senderID, msg)
	}
}

func (s *Server) addClient(channelID, senderID string, stream pb.ChannelRelay_StreamServer) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if _, ok := s.channels[channelID]; !ok {
		s.channels[channelID] = make(map[string]pb.ChannelRelay_StreamServer)
	}

	// Limit to max 2 connections (Senclaw and App Connector)
	if len(s.channels[channelID]) >= 2 && s.channels[channelID][senderID] == nil {
		return fmt.Errorf("channel is full, maximum 2 connections allowed")
	}

	s.channels[channelID][senderID] = stream
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

func (s *Server) broadcast(channelID, senderID string, msg *pb.RelayMessage) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	ch, ok := s.channels[channelID]
	if !ok {
		return
	}

	for id, stream := range ch {
		if id == senderID {
			continue // Do not echo back to sender
		}
		if err := stream.Send(msg); err != nil {
			log.Printf("Failed to send message to %s in channel %s: %v", id, channelID, err)
		}
	}
}
