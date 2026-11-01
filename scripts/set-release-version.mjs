import fs from "node:fs";
import path from "node:path";

const tag = process.env.GITHUB_REF_NAME ?? "";
const match = /^v(\d+\.\d+\.\d+)$/.exec(tag);

if (!match) {
  console.log("No semantic version tag detected; keeping the repository version.");
  process.exit(0);
}

const version = match[1];
const root = process.cwd();

function readJson(file) {
  return JSON.parse(fs.readFileSync(path.join(root, file), "utf8"));
}

function writeJson(file, value) {
  fs.writeFileSync(path.join(root, file), `${JSON.stringify(value, null, 2)}\n`);
}

const packageJson = readJson("package.json");
packageJson.version = version;
writeJson("package.json", packageJson);

const packageLock = readJson("package-lock.json");
packageLock.version = version;
if (packageLock.packages?.[""]) {
  packageLock.packages[""].version = version;
}
writeJson("package-lock.json", packageLock);

for (const file of ["Cargo.toml"]) {
  const fullPath = path.join(root, file);
  const contents = fs.readFileSync(fullPath, "utf8");
  fs.writeFileSync(
    fullPath,
    contents.replace(/^version = ".*"$/m, `version = "${version}"`),
  );
}

const tauriConfigPath = path.join(root, "src-tauri/tauri.conf.json");
const tauriConfig = readJson("src-tauri/tauri.conf.json");
tauriConfig.version = version;
writeJson("src-tauri/tauri.conf.json", tauriConfig);

console.log(`Building StudyBites ${version}`);
