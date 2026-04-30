package main

import (
	"context"
	"log"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	pb "semaclaw/hub-backend/pkg/proto"
)

func main() {
	conn, err := grpc.Dial("localhost:50051", grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("did not connect: %v", err)
	}
	defer conn.Close()
	c := pb.NewChannelRelayClient(conn)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	stream, err := c.Stream(ctx)
	if err != nil {
		log.Fatalf("could not start stream: %v", err)
	}

	// Send a PING message
	req := &pb.RelayMessage{
		ChannelId: "test-channel",
		SenderId:  "client-1",
		Timestamp: time.Now().Unix(),
		Payload: &pb.RelayMessage_Control{
			Control: &pb.ControlMessage{
				Type: pb.ControlMessage_PING,
			},
		},
	}

	if err := stream.Send(req); err != nil {
		log.Fatalf("Failed to send: %v", err)
	}
	log.Printf("Sent PING to channel 'test-channel'")

	// Start receiving
	go func() {
		for {
			res, err := stream.Recv()
			if err != nil {
				log.Printf("Recv error (expected if test finishes): %v", err)
				return
			}
			log.Printf("Received message from %s: %v", res.SenderId, res.Payload)
		}
	}()

	time.Sleep(1 * time.Second)
	log.Println("Connection test successful!")
}
