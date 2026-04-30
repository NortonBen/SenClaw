package main

import (
	"log"
	"net"
	"net/http"
	"os"
	"time"

	"github.com/joho/godotenv"
	"google.golang.org/grpc"

	"semaclaw/hub-backend/internal/api"
	"semaclaw/hub-backend/internal/relay"
	"semaclaw/hub-backend/internal/store"
	pb "semaclaw/hub-backend/pkg/proto"
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

	grpcURL := os.Getenv("GRPC_URL")
	if grpcURL == "" {
		grpcURL = "127.0.0.1:50051"
	}

	// 1. Start REST API on 18080
	go func() {
		restServer := api.NewServer(db, grpcURL)
		log.Println("Starting REST API on :18080")
		if err := http.ListenAndServe(":18080", restServer); err != nil {
			log.Fatalf("failed to serve REST: %v", err)
		}
	}()

	// 2. Start gRPC Server on 50051
	lis, err := net.Listen("tcp", ":50051")
	if err != nil {
		log.Fatalf("failed to listen: %v", err)
	}
	grpcServer := grpc.NewServer()
	relayServer := relay.NewServer(db)

	pb.RegisterChannelRelayServer(grpcServer, relayServer)

	// Start heartbeat monitor
	go relayServer.StartHeartbeat(30 * time.Second)

	log.Println("Starting gRPC Server on :50051")
	if err := grpcServer.Serve(lis); err != nil {
		log.Fatalf("failed to serve gRPC: %v", err)
	}
}
