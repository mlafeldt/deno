// let message: string = "Hello, World!";
// console.log(message);

// await fetch("https://ifconfig.co/json")
//   .then((res) => res.json())
//   .then((json) => console.log(json));

// export {};

import { chromium } from "npm:playwright";
console.log("Taking screenshot...");
const browser = await chromium.launch();
const page = await browser.newPage();
await page.goto("https://www.litprotocol.com");
await page.screenshot({ path: "litprotocol.png" });
await browser.close();
