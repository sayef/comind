package main

import (
	"errors"
	"sync"
)

// ErrOutOfStock is returned when a reservation exceeds what is available.
var ErrOutOfStock = errors.New("out of stock")

// Store keeps stock levels and reservations per SKU.
type Store struct {
	mu       sync.Mutex
	onHand   map[string]int
	reserved map[string]int
}

func NewStore() *Store {
	return &Store{onHand: map[string]int{}, reserved: map[string]int{}}
}

// Available is stock on hand minus open reservations.
func (s *Store) Available(sku string) int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.onHand[sku] - s.reserved[sku]
}

// Reserve holds quantity for an order until it ships or is cancelled.
func (s *Store) Reserve(sku string, quantity int) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.onHand[sku]-s.reserved[sku] < quantity {
		return ErrOutOfStock
	}
	s.reserved[sku] += quantity
	return nil
}

// Release frees a reservation, e.g. when an order is cancelled.
func (s *Store) Release(sku string, quantity int) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.reserved[sku] -= quantity
}
