package main

import (
	"encoding/json"
	"net/http"
)

func routes(s *Store) *http.ServeMux {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/stock/{sku}", s.getStock)
	mux.HandleFunc("POST /api/stock/{sku}/reservations", s.reserve)
	mux.HandleFunc("GET /healthz", func(w http.ResponseWriter, r *http.Request) {})
	return mux
}

func (s *Store) getStock(w http.ResponseWriter, r *http.Request) {
	sku := r.PathValue("sku")
	json.NewEncoder(w).Encode(map[string]int{"available": s.Available(sku)})
}

func (s *Store) reserve(w http.ResponseWriter, r *http.Request) {
	var body struct{ Quantity int }
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	if err := s.Reserve(r.PathValue("sku"), body.Quantity); err != nil {
		http.Error(w, err.Error(), http.StatusConflict)
		return
	}
	w.WriteHeader(http.StatusCreated)
}
