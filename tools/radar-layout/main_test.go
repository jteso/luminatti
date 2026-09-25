package main

import (
	"context"
	"reflect"
	"testing"
)

func TestDeterministicBranchingLayout(t *testing.T) {
	input := request{Sizes: [][2]float64{{248, 70}, {340, 150}, {248, 70}, {248, 70}}, Edges: [][2]int{{0, 1}, {0, 2}, {1, 3}, {2, 3}}}
	first, err := layout(context.Background(), input)
	if err != nil {
		t.Fatal(err)
	}
	second, err := layout(context.Background(), input)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(first, second) {
		t.Fatal("identical graph changed its layout")
	}
	if len(first.Positions) != 4 || len(first.Routes) != 4 {
		t.Fatal("incomplete layout")
	}
	for _, route := range first.Routes {
		if len(route) < 2 {
			t.Fatal("missing route")
		}
		for i := 1; i < len(route); i++ {
			if route[i].X != route[i-1].X && route[i].Y != route[i-1].Y {
				t.Fatal("non-orthogonal route")
			}
		}
	}
}

func TestRejectsInvalidOrOversizedInput(t *testing.T) {
	for _, input := range []request{
		{Sizes: [][2]float64{{0, 70}}},
		{Sizes: [][2]float64{{248, 70}}, Edges: [][2]int{{0, 1}}},
		{Sizes: make([][2]float64, 161)},
		{Edges: make([][2]int, 601)},
	} {
		if _, err := layout(context.Background(), input); err == nil {
			t.Fatal("accepted invalid input")
		}
	}
}
