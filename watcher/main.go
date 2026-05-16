package main

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/evanw/esbuild/pkg/api"
	"github.com/fsnotify/fsnotify"
)

// Helper functions to convert string parameters to ESBuild API constants
func getTarget(target string) api.Target {
	switch strings.ToLower(target) {
	case "es5":
		return api.ES5
	case "es6", "es2015":
		return api.ES2015
	case "es2016":
		return api.ES2016
	case "es2017":
		return api.ES2017
	case "es2018":
		return api.ES2018
	case "es2019":
		return api.ES2019
	case "es2020":
		return api.ES2020
	case "es2021":
		return api.ES2021
	case "es2022":
		return api.ES2022
	case "esnext":
		return api.ESNext
	default:
		return api.ESNext
	}
}

func getPlatform(platform string) api.Platform {
	switch strings.ToLower(platform) {
	case "browser":
		return api.PlatformBrowser
	case "node":
		return api.PlatformNode
	case "neutral":
		return api.PlatformNeutral
	default:
		return api.PlatformBrowser
	}
}

type Config struct {
	Source         string
	Output         string
	Temp           string
	Watch          bool
	NoInitialBuild bool
	Target         string
	Platform       string
	Minify         bool
}

type BuildResult struct {
	Success     bool     `json:"success"`
	Errors      []string `json:"errors,omitempty"`
	Warnings    []string `json:"warnings,omitempty"`
	Files       []string `json:"files"`
	Timestamp   string   `json:"timestamp"`
	BuildTimeMs int64    `json:"build_time_ms"`
}

func main() {
	var config Config

	flag.StringVar(&config.Source, "source", ".", "Source directory to watch")
	flag.StringVar(&config.Output, "output", "dist", "Output directory")
	flag.StringVar(&config.Temp, "temp", "", "Temp directory for intermediate build files (defaults to <source>/.mana-temp)")
	flag.BoolVar(&config.Watch, "watch", false, "Enable watch mode")
	flag.BoolVar(&config.NoInitialBuild, "no-initial-build", false, "Skip the initial build when starting watch mode")
	flag.StringVar(&config.Target, "target", "esnext", "JavaScript target (es5, es6, es2015, es2016, es2017, es2018, es2019, es2020, es2021, es2022, esnext)")
	flag.StringVar(&config.Platform, "platform", "browser", "Target platform (browser, node, neutral)")
	flag.BoolVar(&config.Minify, "minify", true, "Enable minification")
	flag.Parse()

	// Default temp dir is <source>/.mana-temp if not explicitly set
	if config.Temp == "" {
		config.Temp = filepath.Join(config.Source, ".mana-temp")
	}

	// Ensure output directory exists
	if err := os.MkdirAll(config.Output, 0755); err != nil {
		log.Fatalf("Failed to create output directory: %v", err)
	}

	if config.Watch {
		watchAndBuild(config)
	} else {
		buildOnce(config)
	}
}

func buildOnce(config Config) {
	result := performBuild(config)
	outputResult(result)
}

func watchAndBuild(config Config) {
	// Set up file watcher
	watcher, err := fsnotify.NewWatcher()
	if err != nil {
		log.Fatalf("Failed to create watcher: %v", err)
	}
	defer watcher.Close()

	// Add source directory to watcher
	err = filepath.Walk(config.Source, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		// Watch directories and TypeScript/JavaScript files
		if info.IsDir() || strings.HasSuffix(path, ".ts") || strings.HasSuffix(path, ".js") {
			return watcher.Add(path)
		}
		return nil
	})

	if err != nil {
		log.Fatalf("Failed to add paths to watcher: %v", err)
	}

	// Watching for changes - will be handled by Rust logging

	// Perform initial build unless caller already did one
	if !config.NoInitialBuild {
		initialResult := performBuild(config)
		outputResult(initialResult)
	}

	// Debounce mechanism for subsequent changes
	var timer *time.Timer
	debounceDelay := 300 * time.Millisecond

	for {
		select {
		case event, ok := <-watcher.Events:
			if !ok {
				return
			}

			// Only rebuild on write events for relevant files
			if event.Op&fsnotify.Write == fsnotify.Write {
				if strings.HasSuffix(event.Name, ".ts") || strings.HasSuffix(event.Name, ".js") {
					fmt.Printf("File changed: %s\n", event.Name)

					// Reset debounce timer
					if timer != nil {
						timer.Stop()
					}

					timer = time.AfterFunc(debounceDelay, func() {
						result := performBuild(config)
						outputResult(result)
					})
				}
			}

		case err, ok := <-watcher.Errors:
			if !ok {
				return
			}
			fmt.Printf("Watcher error: %v\n", err)
		}
	}
}

func performBuild(config Config) BuildResult {

	// Find all TypeScript/JavaScript entry points
	entryPointPaths, err := findEntryPoints(config.Source)
	if err != nil {
		return BuildResult{
			Success:   false,
			Errors:    []string{fmt.Sprintf("Failed to find entry points: %v", err)},
			Timestamp: time.Now().Format(time.RFC3339),
		}
	}

	if len(entryPointPaths) == 0 {
		return BuildResult{
			Success:   false,
			Errors:    []string{"No entry points found"},
			Timestamp: time.Now().Format(time.RFC3339),
		}
	}

	tempOutput := config.Temp
	os.RemoveAll(tempOutput)
	if err := os.MkdirAll(tempOutput, 0755); err != nil {
		return BuildResult{
			Success:   false,
			Errors:    []string{fmt.Sprintf("Failed to create temp directory: %v", err)},
			Timestamp: time.Now().Format(time.RFC3339),
		}
	}

	// Create entry points with custom output names using ESBuild's EntryPoint struct
	var entryPoints []api.EntryPoint
	var outputFiles []string

	for _, entryPointPath := range entryPointPaths {
		// Generate unique ID for output file (ESBuild will add .js automatically)
		uniqueID := generateUniqueID()

		entryPoints = append(entryPoints, api.EntryPoint{
			InputPath:  entryPointPath,
			OutputPath: uniqueID, // ESBuild adds .js extension automatically
		})
		outputFiles = append(outputFiles, uniqueID+".js")
	}

	// ESBuild configuration with entry points advanced
	buildOptions := api.BuildOptions{
		EntryPointsAdvanced: entryPoints,
		Outdir:              tempOutput,
		Bundle:              true,
		Write:               true,
		Platform:            getPlatform(config.Platform),
		Format:              api.FormatIIFE,
		Target:              getTarget(config.Target),
		MinifyWhitespace:    config.Minify,
		MinifyIdentifiers:   false,
		MinifySyntax:        config.Minify,
		Sourcemap:           api.SourceMapNone,
		LogLevel:            api.LogLevelWarning,
		LegalComments:       api.LegalCommentsNone,
		//DisableEntryPointTail: true,
		GlobalName: "__exports__",
		Footer:     map[string]string{"js": "globalThis.Target=__exports__.Target;"},
	}

	// Perform the build
	buildStart := time.Now()
	buildResult := api.Build(buildOptions)
	buildDuration := time.Since(buildStart)

	// Process results
	var errors []string
	var warnings []string

	for _, err := range buildResult.Errors {
		errors = append(errors, err.Text)
	}

	for _, warning := range buildResult.Warnings {
		warnings = append(warnings, warning.Text)
	}

	// ESBuild should have created files with the custom names directly
	if len(errors) > 0 {
		outputFiles = nil // Clear output files on failure
	}

	result := BuildResult{
		Success:     len(errors) == 0,
		Errors:      errors,
		Warnings:    warnings,
		Files:       outputFiles,
		Timestamp:   time.Now().Format(time.RFC3339),
		BuildTimeMs: buildDuration.Milliseconds(),
	}

	// Build result will be handled by JSON output to Rust

	return result
}

func findEntryPoints(sourceDir string) ([]string, error) {
	var entryPoints []string

	err := filepath.Walk(sourceDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		// Skip common ignore directories
		if info.IsDir() {
			name := info.Name()
			if name == "node_modules" || name == ".git" || name == "dist" || name == "build" {
				return filepath.SkipDir
			}
			return nil
		}

		// Look for TypeScript files that contain 'class Target'
		if strings.HasSuffix(path, ".ts") || strings.HasSuffix(path, ".tsx") {
			content, err := os.ReadFile(path)
			if err != nil {
				fmt.Printf("Warning: Failed to read file %s: %v\n", path, err)
				return nil // Continue processing other files
			}

			if strings.Contains(string(content), "class Target") {
				entryPoints = append(entryPoints, path)
			}
		}

		return nil
	})

	return entryPoints, err
}

func listOutputFiles(outputDir string) ([]string, error) {
	var files []string

	// Read files directly from output directory
	entries, err := os.ReadDir(outputDir)
	if err != nil {
		return files, err
	}

	for _, entry := range entries {
		if !entry.IsDir() && strings.HasSuffix(entry.Name(), ".js") {
			files = append(files, entry.Name())
		}
	}

	return files, nil
}

func generateUniqueID() string {
	bytes := make([]byte, 8) // 8 bytes = 16 hex characters
	rand.Read(bytes)
	return hex.EncodeToString(bytes)
}

func outputResult(result BuildResult) {
	// Output JSON result to stdout for Rust to parse
	jsonData, err := json.Marshal(result)
	if err != nil {
		fmt.Printf("ERROR: Failed to marshal result: %v\n", err)
		return
	}

	// Output JSON result to stdout for Rust to parse (without prefix)
	fmt.Println(string(jsonData))
}
