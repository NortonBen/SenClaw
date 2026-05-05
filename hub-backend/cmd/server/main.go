package main

import (
	"log"
	"net/http"
	"os"
	"time"

	"semaclaw/hub-backend/internal/api"
	"semaclaw/hub-backend/internal/relay"
	"semaclaw/hub-backend/internal/store"

	"github.com/joho/godotenv"
)

func main() {
	// Load .env file if it exists
	if err := godotenv.Load(); err != nil {
		log.Println("No .env file found or error loading, using environment variables")
	}

	// 0. Init Store
	db, err := store.NewStore("hub.db")
	if err != nil {
		log.Fatalf("failed to init db: %v", err)
	}
	defer db.Close()

	// 1. Create HTTP listener
	sharedPort := os.Getenv("SHARED_PORT")
	if sharedPort == "" {
		sharedPort = ":18080"
	}
	// 2. Initialize relay/ws server
	relayServer := relay.NewServer(db)

	// Start heartbeat monitor for relay server
	// Mobile app treats lack of inbound frames as failure; keep this below app wait timeouts.
	go relayServer.StartHeartbeat(10 * time.Second)

	// 3. Initialize HTTP server
	restServer := api.NewServer(db, relayServer)
	httpServer := &http.Server{Addr: sharedPort, Handler: restServer}

	log.Println("Starting REST API on", sharedPort)
	if err := httpServer.ListenAndServe(); err != nil {
		log.Fatalf("server error: %v", err)
	}
}
