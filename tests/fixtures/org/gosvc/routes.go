package main

import "net/http"

func getItem(w http.ResponseWriter, r *http.Request) {}

func routes() *http.ServeMux {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/items/{id}", getItem)
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) {})
	return mux
}

func fetchUser(id string) (*http.Response, error) {
	return http.Get("http://users/api/accounts/" + id)
}
