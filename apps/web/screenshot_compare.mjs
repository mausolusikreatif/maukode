import { chromium } from 'playwright';

async function screenshot(url, label, file) {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 20000 });
  await page.waitForTimeout(2000);
  await page.screenshot({ path: file, fullPage: false });
  const title = await page.title();
  const bodyText = await page.evaluate(() => document.body?.innerText?.slice(0, 500));
  console.log(`=== ${label} ===`);
  console.log(`URL: ${url}`);
  console.log(`Title: ${title}`);
  console.log(`Body text (first 500 chars):`);
  console.log(bodyText);
  console.log('');
  await browser.close();
}

await screenshot('http://100.97.120.54:4096#token=testrahasia', 'msk-1 (local)', '/tmp/msk1.png');
await screenshot('http://100.125.48.96:4096#token=testrahasia', 'msk-server', '/tmp/mskserver.png');
console.log('Screenshots saved: /tmp/msk1.png, /tmp/mskserver.png');
