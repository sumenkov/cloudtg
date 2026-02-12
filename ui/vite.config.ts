import react from "@vitejs/plugin-react";

// Keep plain object export to avoid TS type conflicts when multiple vite versions
// are present in workspace dependency trees during CI installs.
export default {
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true }
};
