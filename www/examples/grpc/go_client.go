// Package main demonstrates a Go gRPC client for tokio_php.
//
// Requirements:
//
//	go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
//	go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest
//
// Generate proto classes:
//
//	protoc --go_out=. --go-grpc_out=. -I../../../proto ../../../proto/php_service.proto
//
// Usage:
//
//	go run go_client.go
package main

import (
	"context"
	"fmt"
	"io"
	"log"
	"os"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

func main() {
	host := os.Getenv("GRPC_HOST")
	if host == "" {
		host = "localhost:50051"
	}

	fmt.Println("=== tokio_php Go gRPC Client ===")
	fmt.Printf("Connecting to: %s\n\n", host)

	// Connect to gRPC server
	conn, err := grpc.NewClient(host,
		grpc.WithTransportCredentials(insecure.NewCredentials()),
	)
	if err != nil {
		log.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	// Note: In production, import generated proto classes
	// For this example, we show the request structure

	fmt.Println("Go gRPC client structure:")
	fmt.Println(`
// Import generated proto
import pb "github.com/your-org/tokio-php-client/proto"

// Create client
client := pb.NewPhpServiceClient(conn)

// Execute request
req := &pb.ExecuteRequest{
    ScriptPath:  "index.php",
    Method:      "GET",
    QueryParams: map[string]string{"page": "1"},
    Options: &pb.RequestOptions{
        TimeoutMs: 5000,
    },
}

// Call service
resp, err := client.Execute(ctx, req)
if err != nil {
    log.Fatal(err)
}

fmt.Printf("Status: %d\n", resp.StatusCode)
fmt.Printf("Body: %s\n", string(resp.Body))
`)

	// Raw example (without generated code)
	fmt.Println("\nRaw gRPC example (without proto generation):")
	rawExample(ctx, conn)
}

func rawExample(ctx context.Context, conn *grpc.ClientConn) {
	// This demonstrates the structure without generated code
	// In production, always use protoc-generated classes

	fmt.Println(`
// Health check (raw)
stream, err := conn.NewStream(ctx, &grpc.StreamDesc{
    StreamName: "Check",
}, "/tokio_php.v1.PhpService/Check")

// Execute (raw)
stream, err := conn.NewStream(ctx, &grpc.StreamDesc{
    StreamName: "Execute",
}, "/tokio_php.v1.PhpService/Execute")
`)
}
