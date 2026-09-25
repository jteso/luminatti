// Geometry-only bridge to D2's MPL-2.0 TALA engine. No source text, paths,
// D2 scripts, remote assets, or repository code are evaluated here.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"math"
	"os"
	"sort"
	"time"

	"github.com/d2lang/d2/d2graph"
	"github.com/d2lang/d2/d2layouts/d2talalayout"
	"github.com/d2lang/d2/lib/geo"
)

type request struct {
	Sizes [][2]float64 `json:"sizes"`
	Edges [][2]int     `json:"edges"`
}
type point struct {
	X float64 `json:"x"`
	Y float64 `json:"y"`
}
type result struct {
	Positions []point   `json:"positions"`
	Routes    [][]point `json:"routes"`
}

func layoutConnected(ctx context.Context, input request) (result, error) {
	output := result{Positions: []point{}, Routes: [][]point{}}
	if len(input.Sizes) > 160 || len(input.Edges) > 600 {
		return output, fmt.Errorf("diagram exceeds interactive layout budget")
	}
	g := d2graph.NewGraph()
	g.Root.Attributes.Direction.Value = "down"
	for i, size := range input.Sizes {
		if math.IsNaN(size[0]) || math.IsNaN(size[1]) || size[0] < 1 || size[1] < 1 || size[0] > 100000 || size[1] > 100000 {
			return output, fmt.Errorf("invalid node size")
		}
		id := fmt.Sprintf("n%d", i)
		o := &d2graph.Object{
			Graph: g, Parent: g.Root, ID: id, IDVal: id,
			Box:      geo.NewBox(geo.NewPoint(0, 0), size[0], size[1]),
			Children: make(map[string]*d2graph.Object),
		}
		g.Root.Children[id] = o
		g.Root.ChildrenArray = append(g.Root.ChildrenArray, o)
		g.Objects = append(g.Objects, o)
	}
	for _, pair := range input.Edges {
		if pair[0] < 0 || pair[1] < 0 || pair[0] >= len(g.Objects) || pair[1] >= len(g.Objects) || pair[0] == pair[1] {
			return output, fmt.Errorf("invalid edge")
		}
		g.Edges = append(g.Edges, &d2graph.Edge{Src: g.Objects[pair[0]], Dst: g.Objects[pair[1]], DstArrow: true})
	}
	// Fixed seeds keep an identical graph reproducible across refreshes.
	opts := d2talalayout.DefaultOptions()
	opts.MaxConcurrency = 2
	objects := append([]*d2graph.Object(nil), g.Objects...)
	edges := append([]*d2graph.Edge(nil), g.Edges...)
	if err := d2talalayout.Layout(ctx, g, &opts); err != nil {
		return output, err
	}
	for _, object := range objects {
		output.Positions = append(output.Positions, point{object.TopLeft.X, object.TopLeft.Y})
	}
	for _, edge := range edges {
		route := []point{}
		for _, p := range edge.Route {
			route = append(route, point{p.X, p.Y})
		}
		output.Routes = append(output.Routes, route)
	}
	return output, nil
}

// Pack weak components by their complete route bounds. TALA can interleave
// disconnected objects, so laying out each component separately prevents an
// independent tree from covering another tree's arrows.
func layout(ctx context.Context, input request) (result, error) {
	output := result{Positions: make([]point, len(input.Sizes)), Routes: make([][]point, len(input.Edges))}
	if len(input.Sizes) > 160 || len(input.Edges) > 600 {
		return output, fmt.Errorf("diagram exceeds interactive layout budget")
	}
	adjacent := make([][]int, len(input.Sizes))
	for _, edge := range input.Edges {
		if edge[0] < 0 || edge[1] < 0 || edge[0] >= len(adjacent) || edge[1] >= len(adjacent) || edge[0] == edge[1] {
			return output, fmt.Errorf("invalid edge")
		}
		adjacent[edge[0]] = append(adjacent[edge[0]], edge[1])
		adjacent[edge[1]] = append(adjacent[edge[1]], edge[0])
	}
	seen := make([]bool, len(adjacent))
	x, y, bottom := 0.0, 0.0, 0.0
	for start := range adjacent {
		if seen[start] {
			continue
		}
		seen[start] = true
		members := []int{start}
		for i := 0; i < len(members); i++ {
			for _, next := range adjacent[members[i]] {
				if !seen[next] {
					seen[next] = true
					members = append(members, next)
				}
			}
		}
		sort.Ints(members)
		local := request{}
		indices := make(map[int]int)
		for i, member := range members {
			indices[member] = i
			local.Sizes = append(local.Sizes, input.Sizes[member])
		}
		edgeIndices := []int{}
		for i, edge := range input.Edges {
			if from, ok := indices[edge[0]]; ok {
				local.Edges = append(local.Edges, [2]int{from, indices[edge[1]]})
				edgeIndices = append(edgeIndices, i)
			}
		}
		geometry, err := layoutConnected(ctx, local)
		if err != nil {
			return output, err
		}
		minX, minY, maxX, maxY := math.Inf(1), math.Inf(1), math.Inf(-1), math.Inf(-1)
		include := func(p point) {
			minX = math.Min(minX, p.X)
			minY = math.Min(minY, p.Y)
			maxX = math.Max(maxX, p.X)
			maxY = math.Max(maxY, p.Y)
		}
		for i, p := range geometry.Positions {
			include(p)
			include(point{p.X + local.Sizes[i][0], p.Y + local.Sizes[i][1]})
		}
		for _, route := range geometry.Routes {
			for _, p := range route {
				include(p)
			}
		}
		width, height := maxX-minX, maxY-minY
		if x > 0 && x+width > 1100 {
			x = 0
			y = bottom + 64
		}
		translate := func(p point) point { return point{p.X - minX + x, p.Y - minY + y} }
		for i, p := range geometry.Positions {
			output.Positions[members[i]] = translate(p)
		}
		for i, route := range geometry.Routes {
			for _, p := range route {
				output.Routes[edgeIndices[i]] = append(output.Routes[edgeIndices[i]], translate(p))
			}
		}
		x += width + 64
		bottom = math.Max(bottom, y+height)
	}
	return output, nil
}

func main() {
	var input request
	if err := json.NewDecoder(io.LimitReader(os.Stdin, 65536)).Decode(&input); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Second)
	defer cancel()
	output, err := layout(ctx, input)
	if err == nil {
		err = json.NewEncoder(os.Stdout).Encode(output)
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
