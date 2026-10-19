package main

import (
	"log"
	"net/http"
)

func main() {
	store := NewStore()
	log.Fatal(http.ListenAndServe(":8080", routes(store)))
}
