// Builds rust/assets/ubo-resources.json from a copy of uBlock Origin's
// source: its scriptlets (`+js(...)` filters) and redirect resources
// (`$redirect=...`), in the format of Brave's adblock-rust.
//
//   node tools/ubo_resources.mjs <uBlock source folder> <output .json>
//
// The source comes from https://github.com/gorhill/uBlock/releases (a tag's
// "Source code" archive). uBlock Origin is GPL-3.0, as Idlerx.

import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const [src, out] = process.argv.slice(2);
if (!src || !out) {
    console.error('usage: node tools/ubo_resources.mjs <uBlock source folder> <output .json>');
    process.exit(1);
}
const js = (p) => pathToFileURL(path.join(src, 'src', 'js', p)).href;
const b64 = (data) => Buffer.from(data).toString('base64');
// adblock-rust's permission bit for uBlock's "trusted" scriptlets (only the
// uBlock lists may use them, as in uBlock Origin)
const TRUSTED = 1;

const resources = [];

const { builtinScriptlets } = await import(js('resources/scriptlets.js'));
for (const s of builtinScriptlets) {
    // uBlock's isolated-world scriptlets need its extension: not usable here
    if (s.world === 'ISOLATED') continue;
    resources.push({
        name: s.name,
        aliases: s.aliases ?? [],
        kind: { mime: s.name.endsWith('.fn') ? 'fn/javascript' : 'application/javascript' },
        content: b64(s.fn.toString()),
        dependencies: s.dependencies ?? [],
        permission: s.requiresTrust ? TRUSTED : 0,
    });
}

const mime = (name) => ({
    '.js': 'application/javascript',
    '.html': 'text/html',
    '.txt': 'text/plain',
    '.css': 'text/css',
    '.json': 'application/json',
    '.gif': 'image/gif',
    '.png': 'image/png',
    '.mp3': 'audio/mp3',
    '.mp4': 'video/mp4',
    '.xml': 'text/xml',
}[path.extname(name)] ?? 'application/octet-stream');

const { default: redirects } = await import(js('redirect-resources.js'));
const known = new Set(resources.map((r) => r.name));
for (const [name, details] of redirects) {
    if (known.has(name)) continue;
    const file = path.join(src, 'src', 'web_accessible_resources', name);
    if (!fs.existsSync(file)) continue;
    const alias = details.alias;
    resources.push({
        name,
        aliases: alias === undefined ? [] : Array.isArray(alias) ? alias : [alias],
        kind: { mime: mime(name) },
        content: b64(fs.readFileSync(file)),
        dependencies: [],
        permission: 0,
    });
}

fs.writeFileSync(out, JSON.stringify(resources));
console.log(`${resources.length} resources written to ${out}`);
