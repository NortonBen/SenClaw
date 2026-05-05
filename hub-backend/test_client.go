package main

import (
	"encoding/json"
	"log"
	"time"

	"github.com/gorilla/websocket"
)

func main() {
	conn, _, err := websocket.DefaultDialer.Dial("ws://localhost:18080/v1/relay/ws?channel_id=test-channel&access_token=test-token", nil)
	if err != nil {
		log.Fatalf("did not connect: %v", err)
	}
	defer conn.Close()

	req := map[string]any{
		"type":       "ping",
		"channel_id": "test-channel",
		"sender_id":  "client-1",
		"timestamp":  time.Now().UnixMilli(),
		"message_id": "test-ping",
	}
	raw, _ := json.Marshal(req)
	if err := conn.WriteMessage(websocket.TextMessage, raw); err != nil {
		log.Fatalf("Failed to send: %v", err)
	}
	log.Printf("Sent JSON ping frame")

	go func() {
		for {
			_, res, err := conn.ReadMessage()
			if err != nil {
				log.Printf("Read error: %v", err)
				return
			}
			log.Printf("Received frame: %s", string(res))
		}
	}()

	time.Sleep(3 * time.Second)
	log.Println("WS connection test done")
}
