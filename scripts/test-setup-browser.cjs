const {chromium}=require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
(async()=>{
 const browser=await chromium.launch({headless:true,...(process.env.BROWSER_CHANNEL?{channel:process.env.BROWSER_CHANNEL}:{})});
 try {
  for(const width of [390,1280]) for(const installed of [true,false]) {
   const page=await browser.newPage({viewport:{width,height:1000}}),errors=[];
   page.on('pageerror',e=>errors.push(e.message));
   await page.route('http://wallet.test/**',async route=>{
    const url=new URL(route.request().url());
    if(url.pathname.startsWith('/api/')) {
     assert.equal(route.request().method(),'GET','Setup inspection must not create a wallet or send payments');
     const value=url.pathname==='/api/v1/wallet'?{fingerprint:null}:url.pathname==='/api/config'?{arkServer:installed?'https://ark.paperclippool.xyz':'https://operator.example'}:url.pathname==='/api/v1/wallet/platform-node'?{available:installed}:{};
     return route.fulfill({json:value});
    }
    const file=path.join(__dirname,'../web',url.pathname==='/'?'index.html':url.pathname.slice(1));
    return route.fulfill({body:fs.readFileSync(file),contentType:file.endsWith('.js')?'application/javascript':file.endsWith('.css')?'text/css':file.endsWith('.html')?'text/html':'image/jpeg'});
   });
   await page.goto('http://wallet.test/');
   await page.locator('#token').fill('test-token');
   await page.getByRole('button',{name:'Unlock wallet',exact:true}).click();
   await page.locator('#setup').waitFor({state:'visible'});
   await page.waitForFunction(()=>document.querySelector('#setup-backend').value===document.querySelector('#setup-installed').disabled.toString().replace('true','manual').replace('false','installed'));
   assert.equal(await page.locator('#setup-asp').inputValue(),installed?'https://ark.paperclippool.xyz':'https://operator.example');
   await page.locator('#setup-asp').fill('https://custom.example');
   await page.locator('#setup-backend').selectOption('manual');
   assert.equal(await page.locator('#setup-asp').inputValue(),'https://custom.example');
   assert(await page.locator('#setup-rpc').isVisible());
   const checkbox=await page.locator('#setup-ack').boundingBox();
   assert(checkbox.width<=24 && checkbox.height<=24,'Checkbox must remain compact');
   const ack=await page.locator('.setup-ack').boundingBox(),buttons=await page.locator('.setup-actions').boundingBox();
   assert(buttons.y>=ack.y+ack.height,'Buttons must be below acknowledgement');
   assert(!(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth)),'No horizontal overflow');
   assert.deepEqual(errors,[]);
   if(process.env.SCREENSHOT_DIR) await page.screenshot({path:path.join(process.env.SCREENSHOT_DIR,`setup-${width}-${installed}.png`),fullPage:true});
   await page.close();
  }
  console.log('PASS browser setup: mobile/desktop, installed/manual node, ASP default/edit retention, compact checkbox, separate actions, no overflow/errors');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
