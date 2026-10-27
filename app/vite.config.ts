import path from "node:path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

// Tauri loads http://localhost:5173 in dev and ../dist in a build (app/src-tauri/tauri.conf.json).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@ui": path.resolve(import.meta.dirname, "./src/ui"), "@": path.resolve(import.meta.dirname, "./src") } },
  clearScreen: false,
  server: { port: 5173, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  // MapLibre starts its worker as a module worker (src/components/debrief/FlightMap.tsx).
  worker: { format: "es" },
  build: { target: "es2022", chunkSizeWarningLimit: 1200 },
})
