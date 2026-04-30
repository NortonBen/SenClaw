package main

import (
	"log"
	"net"
	"net/http"
	"os"
	"time"

	"semaclaw/hub-backend/internal/api"
	"semaclaw/hub-backend/internal/relay"
	"semaclaw/hub-backend/internal/store"
	pb "semaclaw/hub-backend/pkg/proto"

	"github.com/joho/godotenv"
	"github.com/soheilhy/cmux"
	"golang.org/x/sync/errgroup"
	"google.golang.org/grpc"
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

	log.Println("gRPC URL: ", grpcURL)

	// 1. Create a shared listener for both HTTP and gRPC
	sharedPort := os.Getenv("SHARED_PORT")
	if sharedPort == "" {
		sharedPort = ":18080"
	}
	lis, err := net.Listen("tcp", sharedPort)
	if err != nil {
		log.Fatalf("failed to listen on %s: %v", sharedPort, err)
	}

	// 2. Set up connection multiplexer
	m := cmux.New(lis)
	grpcL := m.MatchWithWriters(cmux.HTTP2MatchHeaderFieldSendSettings("content-type", "application/grpc"))
	httpL := m.Match(cmux.Any())

	// 3. Initialize gRPC server and register services
	grpcServer := grpc.NewServer()
	relayServer := relay.NewServer(db)
	pb.RegisterChannelRelayServer(grpcServer, relayServer)

	// Start heartbeat monitor for relay server
	go relayServer.StartHeartbeat(30 * time.Second)

	// 4. Initialize HTTP server
	restServer := api.NewServer(db, grpcURL)
	httpServer := &http.Server{Handler: restServer}

	// 5. Run servers concurrently using errgroup
	var g errgroup.Group

	g.Go(func() error {
		log.Println("Starting REST API on shared port", sharedPort)
		return httpServer.Serve(httpL)
	})

	g.Go(func() error {
		log.Println("Starting gRPC Server on shared port", sharedPort)
		return grpcServer.Serve(grpcL)
	})

	g.Go(func() error {
		return m.Serve()
	})

	if err := g.Wait(); err != nil {
		log.Fatalf("server error: %v", err)
	}
}
