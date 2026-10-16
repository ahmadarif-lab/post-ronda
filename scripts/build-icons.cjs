// Development-only. Runtime uses the frozen transparent atlas, no SVG engine.
const fs=require('node:fs/promises');
const sharp=require(process.env.SHARP_MODULE || 'sharp');
const names=['home','cube','search','plus','dots-vertical','git-branch','download','upload','code','device-floppy','shield-check','eye','trash','chevron-right','settings','book','history','palette','layout-sidebar','layout-columns','x','folder','world','copy','send','pencil','info-circle','dots','pin','sun','moon','device-desktop','layout-rows'];
(async()=>{
  const tiles=await Promise.all(names.map(async(name,i)=>{
    const path=`assets/tabler/${name}.svg`;
    let svg;try{svg=await fs.readFile(path,'utf8');}catch{
      const r=await fetch(`https://raw.githubusercontent.com/tabler/tabler-icons/main/icons/outline/${name}.svg`);if(!r.ok)throw Error(`${name}: ${r.status}`);svg=await r.text();await fs.writeFile(path,svg);
    }
    svg=svg.replace(/currentColor/g,'#ffffff').replace('stroke-width="2"','stroke-width="1.5"');
    return {input:await sharp(Buffer.from(svg)).resize(48,48).png().toBuffer(),left:(i%8)*64+8,top:Math.floor(i/8)*64+8};
  }));
  // The empty response pane draws the send icon at 140pt; a 48px atlas tile would be blurry there.
  const big=(await fs.readFile('assets/tabler/send.svg','utf8')).replace(/currentColor/g,'#ffffff').replace('stroke-width="2"','stroke-width="1.2"');
  await sharp(Buffer.from(big),{density:1800}).resize(420,420).png({compressionLevel:9}).toFile('assets/send-large.png');
  await sharp({create:{width:512,height:320,channels:4,background:{r:0,g:0,b:0,alpha:0}}}).composite(tiles).png().toFile('assets/tabler-atlas.png');
})();
