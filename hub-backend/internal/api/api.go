package api

import (
	"encoding/json"
	"net/http"

	"semaclaw/hub-backend/internal/models"
	"semaclaw/hub-backend/internal/store"
)

type Server struct {
	mux     *http.ServeMux
	store   *store.Store
	grpcURL string
}

func NewServer(store *store.Store, grpcURL string) *Server {
	s := &Server{
		mux:     http.NewServeMux(),
		store:   store,
		grpcURL: grpcURL,
	}
	s.routes()
	return s
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	// Simple CORS
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
	w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")

	if r.Method == "OPTIONS" {
		w.WriteHeader(http.StatusOK)
		return
	}

	s.mux.ServeHTTP(w, r)
}

func (s *Server) routes() {
	// Channel Sync API
	s.mux.HandleFunc("GET /v1/agents", s.handleGetAgents())
	s.mux.HandleFunc("GET /v1/messages/{agentId}", s.handleGetMessages())

	// Registry API
	s.mux.HandleFunc("GET /v1/search", s.handleSearch())
	s.mux.HandleFunc("GET /v1/skills/{slug}", s.handleGetSkill())
	s.mux.HandleFunc("GET /v1/download", s.handleDownload())
	s.mux.HandleFunc("GET /v1/resolve", s.handleResolve())
	s.mux.HandleFunc("GET /v1/whoami", s.handleWhoami())
	s.mux.HandleFunc("POST /v1/skills", s.handlePublishSkill())

	// Channels Auth API
	s.mux.HandleFunc("POST /v1/channels/register", s.handleRegisterChannel())
	s.mux.HandleFunc("POST /v1/channels/verify", s.handleVerifyChannel())
}

func (s *Server) handleGetAgents() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		agents := []models.Agent{
			{ID: "agent-1", Name: "SemaClaw Default", Status: "idle"},
		}
		json.NewEncoder(w).Encode(agents)
	}
}

func (s *Server) handleGetMessages() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		agentId := r.PathValue("agentId")
		// MOCK
		messages := []models.Message{
			{ID: "msg-1", Role: "agent", Content: "Hello from " + agentId, IsEncrypted: false},
		}
		json.NewEncoder(w).Encode(messages)
	}
}

func (s *Server) handleSearch() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query().Get("q")
		results := map[string]interface{}{
			"results": []models.SearchResult{
				{Slug: "mock-skill", DisplayName: "Mock Skill for " + q, Score: 1.0},
			},
		}
		json.NewEncoder(w).Encode(results)
	}
}

func (s *Server) handleGetSkill() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		slug := r.PathValue("slug")
		meta := models.SkillMeta{
			Skill: models.SkillInfo{Slug: slug, DisplayName: "Skill " + slug},
		}
		json.NewEncoder(w).Encode(meta)
	}
}

func (s *Server) handleDownload() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Write([]byte("mock zip content"))
	}
}

func (s *Server) handleResolve() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		json.NewEncoder(w).Encode(map[string]interface{}{
			"match": map[string]string{"version": "1.0.0"},
		})
	}
}

func (s *Server) handleWhoami() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		json.NewEncoder(w).Encode(map[string]interface{}{
			"user": map[string]string{"handle": "admin", "displayName": "Admin"},
		})
	}
}

func (s *Server) handlePublishSkill() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		json.NewEncoder(w).Encode(map[string]interface{}{
			"skill": map[string]string{"slug": "new-skill", "version": "1.0.1"},
		})
	}
}

func (s *Server) handleRegisterChannel() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		channelID, token, err := s.store.RegisterChannel()
		if err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}
		
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{
			"channel_id":   channelID,
			"access_token": token,
		})
	}
}

func (s *Server) handleVerifyChannel() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var req struct {
			ChannelID   string `json:"channel_id"`
			AccessToken string `json:"access_token"`
		}
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, "invalid request", http.StatusBadRequest)
			return
		}

		valid := s.store.VerifyChannel(req.ChannelID, req.AccessToken)
		w.Header().Set("Content-Type", "application/json")
		
		if valid {
			json.NewEncoder(w).Encode(map[string]interface{}{
				"valid":    true,
				"grpc_url": s.grpcURL,
			})
		} else {
			json.NewEncoder(w).Encode(map[string]interface{}{
				"valid": false,
			})
		}
	}
}
