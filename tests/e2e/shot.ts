// Ad-hoc: `bun tests/e2e/shot.ts <select-path> [out.png]` — one screenshot of a state.
import { launch, ROOT, screenshot } from "./harness";
import { join } from "node:path";

const [sel = "drcode/file-browser/temp_0.md", out = join(ROOT, "screenshots/current/adhoc.png")] = process.argv.slice(2);
const app = await launch({ select: sel, root: process.env.LUPASTA_ROOT }); // LUPASTA_ROOT: real folder instead of the fixture
try {
  await screenshot(app.page, out);
  console.log(out);
} finally {
  await app.close();
}
