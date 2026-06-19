// src/lib/file-preview.test.ts
import test from "node:test";
import assert from "node:assert/strict";

// src/lib/file-preview.ts
var FILE_PREVIEW_LIMIT_BYTES = 128 * 1024;
function shouldFetchPreviewContent(entry) {
  if (entry.is_dir) return false;
  return entry.type === "text" || entry.type === "json";
}

// src/lib/file-preview.test.ts
test("shouldFetchPreviewContent only fetches text-like file content", () => {
  assert.equal(shouldFetchPreviewContent({ is_dir: false, type: "text" }), true);
  assert.equal(shouldFetchPreviewContent({ is_dir: false, type: "json" }), true);
  assert.equal(shouldFetchPreviewContent({ is_dir: false, type: "image" }), false);
  assert.equal(shouldFetchPreviewContent({ is_dir: false, type: "audio" }), false);
  assert.equal(shouldFetchPreviewContent({ is_dir: false, type: "video" }), false);
  assert.equal(shouldFetchPreviewContent({ is_dir: false, type: "pdf" }), false);
  assert.equal(shouldFetchPreviewContent({ is_dir: true, type: "directory" }), false);
});
test("FILE_PREVIEW_LIMIT_BYTES keeps DOM previews bounded", () => {
  assert.equal(FILE_PREVIEW_LIMIT_BYTES, 128 * 1024);
});
