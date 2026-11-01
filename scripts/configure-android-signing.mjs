import fs from "node:fs";
import path from "node:path";

const required = ["ANDROID_KEYSTORE_PATH", "ANDROID_KEY_ALIAS", "ANDROID_KEY_PASSWORD"];
const missing = required.filter((name) => !process.env[name]);

if (missing.length > 0) {
  throw new Error(`Missing Android signing environment variables: ${missing.join(", ")}`);
}

const androidRoot = path.resolve("src-tauri/gen/android");
const gradlePath = path.join(androidRoot, "app/build.gradle.kts");
const propertiesPath = path.join(androidRoot, "keystore.properties");
let gradle = fs.readFileSync(gradlePath, "utf8");

if (!gradle.includes('signingConfigs.create("release")')) {
  const signingBlock = `
    signingConfigs {
        create("release") {
            val keystorePropertiesFile = rootProject.file("keystore.properties")
            val keystoreProperties = Properties()
            keystorePropertiesFile.inputStream().use { keystoreProperties.load(it) }
            keyAlias = keystoreProperties["keyAlias"] as String
            keyPassword = keystoreProperties["keyPassword"] as String
            storeFile = file(keystoreProperties["storeFile"] as String)
            storePassword = keystoreProperties["storePassword"] as String
        }
    }
`;
  gradle = gradle.replace("    buildTypes {", `${signingBlock}\n    buildTypes {`);
}

const releaseBlock = `        getByName("release") {
            signingConfig = signingConfigs.getByName("release")
`;
if (!gradle.includes('signingConfig = signingConfigs.getByName("release")')) {
  gradle = gradle.replace(
    '        getByName("release") {\n',
    releaseBlock,
  );
}

fs.writeFileSync(gradlePath, gradle);
fs.writeFileSync(
  propertiesPath,
  [
    `keyAlias=${process.env.ANDROID_KEY_ALIAS}`,
    `keyPassword=${process.env.ANDROID_KEY_PASSWORD}`,
    `storeFile=${process.env.ANDROID_KEYSTORE_PATH}`,
    `storePassword=${process.env.ANDROID_KEY_PASSWORD}`,
    "",
  ].join("\n"),
);
