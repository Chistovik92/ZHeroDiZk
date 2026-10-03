import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Relative base: the built panel can be served from any path. In development the API of a local
// control server (default 127.0.0.1:21114) is proxied so that no CORS setup is needed.
export default defineConfig({
  base: "./",
  plugins: [react()],
  server: {
    proxy: {
      "/v1": "http://127.0.0.1:21114",
      "/healthz": "http://127.0.0.1:21114",
    },
  },
  test: { environment: "node" },
});
