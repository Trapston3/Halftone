// dump console errors from the main page inside the harness
const CHROME=process.env.HOME+"/.cache/ms-playwright/chromium-1134/chrome-linux/chrome";
const {spawn}=require("child_process");
const url=process.argv[2]||"file:///home/ojesus/MP_App/ht-ui/ui/test/ui_harness.html?page=main&nav=left";
const out=process.argv[3]||"/tmp/halftone_console.txt";
const cdp=require("child_process").exec;
// use --enable-logging to capture console
const args=["--headless=new","--no-sandbox","--disable-gpu","--enable-logging=stderr","--v=0",
  "--virtual-time-budget=6000","--dump-dom",url];
const p=spawn(CHROME,args,{stdio:["ignore","ignore","pipe"]});
let err="";
p.stderr.on("data",d=>err+=d.toString());
p.on("close",()=>{
  const lines=err.split("\n").filter(l=>l.includes("CONSOLE")||l.includes("Uncaught")||l.includes("ERROR"));
  require("fs").writeFileSync(out,lines.join("\n"));
  console.log("console lines:",lines.length,"written to",out);
});
