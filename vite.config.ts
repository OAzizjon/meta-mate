import { defineConfig } from "vite";

// One page per window: Mate and the click-through highlight (src-tauri/src/overlay.rs).
export default defineConfig({
  // Keep Rust errors visible in the terminal.
  clearScreen: false,
  server: {
    // Tauri's devUrl expects exactly this port.
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        mate: "mate.html",
        highlight: "highlight.html",
        s3: "s3.html",
        debug: "debug.html",
        settings: "settings.html",
        card: "card.html",
        home: "home.html",
        chat: "chat.html",
        signin: "signin.html",
      },
    },
  },
});
