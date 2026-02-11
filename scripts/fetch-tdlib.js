const { execFileSync } = require("node:child_process");
const { platform } = require("node:process");
const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");

const root = path.resolve(__dirname, "..");

function run(cmd, args) {
  execFileSync(cmd, args, { stdio: "inherit", cwd: root });
}

function fileExists(p) {
  try {
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
}

function detectExistingTdlib() {
  const osId = platform === "win32" ? "windows" : platform === "darwin" ? "macos" : "linux";
  const archId = os.arch() === "arm64" ? "aarch64" : os.arch() === "x64" ? "x86_64" : os.arch();
  const names =
    platform === "win32" ? ["tdjson.dll"] : platform === "darwin" ? ["libtdjson.dylib"] : ["libtdjson.so", "libtdjson.so.1"];
  const bases = [
    path.join(root, "src-tauri", "resources", "tdlib", `${osId}-${archId}`),
    path.join(root, "src-tauri", "resources", "tdlib", osId),
    path.join(root, "src-tauri", "resources", "tdlib")
  ];
  for (const base of bases) {
    for (const name of names) {
      const libPath = path.join(base, name);
      if (!fileExists(libPath)) continue;

      // On Windows the prebuilt tdjson.dll usually needs extra runtime DLLs (OpenSSL, zlib).
      // If only tdjson.dll is present, treat it as "not installed" and fetch a complete bundle.
      if (platform === "win32" && name === "tdjson.dll") {
        try {
          const files = fs
            .readdirSync(base, { withFileTypes: true })
            .filter((e) => e.isFile())
            .map((e) => e.name.toLowerCase());
          const hasCrypto = files.some((n) => n.startsWith("libcrypto") && n.endsWith(".dll"));
          const hasSsl = files.some((n) => n.startsWith("libssl") && n.endsWith(".dll"));
          const hasZlib = files.includes("zlib1.dll");
          if (!hasCrypto || !hasSsl || !hasZlib) {
            continue;
          }
        } catch {
          continue;
        }
      }

      return libPath;
    }
  }
  return null;
}

const existing = detectExistingTdlib();
if (existing) {
  console.log(`TDLib уже есть: ${existing}`);
  process.exit(0);
}

if (platform === "win32") {
  run("powershell", [
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    path.join("scripts", "fetch-tdlib.ps1")
  ]);
} else {
  run("bash", [path.join("scripts", "fetch-tdlib.sh")]);
}
