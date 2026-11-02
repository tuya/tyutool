/** Export the selected logo from its SVG master, without redrawing any geometry. */
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const directory = new URL("../brand/", import.meta.url);
const master = await readFile(
  new URL("tyutool-data-write-logo.svg", directory),
  "utf8",
);
const browser = await chromium.launch({ headless: true });

try {
  const page = await browser.newPage({
    viewport: { width: 1024, height: 1024 },
  });
  const icon = await page.evaluate((source) => {
    const doc = new DOMParser().parseFromString(source, "image/svg+xml");
    if (doc.querySelector("parsererror")) throw new Error("Invalid logo SVG");
    const symbol = doc.querySelector("#tyutool-mark");
    if (!symbol) throw new Error("The logo must define #tyutool-mark");
    const viewBox = symbol.getAttribute("viewBox");
    return `<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="${viewBox}" role="img" aria-labelledby="title desc">
  <title id="title">tyutool</title>
  <desc id="desc">An orange data block aligned with a dark receiving socket.</desc>
  <!-- Generated from tyutool-data-write-logo.svg; edit the master instead. -->
${symbol.innerHTML.trim()}
</svg>\n`;
  }, master);
  await writeFile(new URL("tyutool-data-write-icon.svg", directory), icon);

  for (const [name, svg] of [
    ["logo", master],
    ["icon", icon],
  ]) {
    await page.setContent(
      `<style>html,body{margin:0;width:100%;height:100%;background:transparent}body>svg{display:block;width:100%;height:100%}</style>${svg}`,
    );
    await page.locator("body > svg").screenshot({
      path: fileURLToPath(new URL(`tyutool-data-write-${name}.png`, directory)),
      omitBackground: true,
    });
  }
  console.log(
    "Exported standalone SVG and two 1024 × 1024 transparent PNGs from the SVG master.",
  );
} finally {
  await browser.close();
}
