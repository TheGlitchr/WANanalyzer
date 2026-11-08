(() => {
  'use strict';
  const provider = __PROVIDER__;
  const hosts = {WiFiman:['wifiman.com'], Speedtest:['speedtest.net','www.speedtest.net'], Fast:['fast.com','www.fast.com'], Devina:['devina.io']};
  const empty = {origin:location.origin,text:'',active:false,complete:false,phase:'Loading provider',fields:{}};
  if (!hosts[provider].includes(location.hostname) || location.protocol !== 'https:') return empty;
  const state = window.__wanalyzerAdapterV2 ||= {started:false,seenActive:false,details:false,attempts:0,clickedAt:0};
  const visible = el => !!el && !!el.getClientRects().length && getComputedStyle(el).visibility !== 'hidden' && getComputedStyle(el).display !== 'none';
  const text = el => (el?.innerText || el?.textContent || '').trim();
  const val = id => text(document.getElementById(id));
  const cls = (id, name) => document.getElementById(id)?.classList.contains(name) || false;
  const buttons = () => Array.from(document.querySelectorAll('button,[role="button"],a'));
  const exactButton = re => buttons().find(el => visible(el) && !el.disabled && re.test(text(el)));
  const body = document.body?.innerText || '';
  const fields = {};
  let active = false, complete = false, phase = 'Preparing test';
  const clickStart = el => {
    if (!el) return false;
    el.click(); state.started = true; state.seenActive = false; state.attempts++; state.clickedAt = Date.now(); active = true;
    return true;
  };
  // Do not solve challenges or make privacy/consent choices for the user.
  const challenge = /verify (?:that )?you are human|checking your browser|just a moment|complete the security check/i.test(body);
  const consent = Array.from(document.querySelectorAll('[role="dialog"],[aria-modal="true"]')).find(el => visible(el) && /cookie|privacy|consent/i.test(text(el)));
  if (challenge || consent) return {...empty,text:body.slice(0,32768),phase:'Action required: show provider page to continue'};
  if (provider === 'WiFiman') {
    active = /Starting test|Stop test/.test(body);
    complete = !active && /Test Result/.test(body);
    if (active) state.started = true;
    if (!state.started && !complete) clickStart(exactButton(/^Start (?:speed )?test$/i));
    phase = active ? 'Measuring throughput and latency' : 'Reading completed result';
  } else if (provider === 'Fast') {
    active = cls('speed-progress-indicator','in-progress');
    if (active) state.started = true;
    const downloadReady = cls('speed-value','succeeded');
    if (downloadReady && !state.details && visible(document.getElementById('show-more-details-link'))) {
      document.getElementById('show-more-details-link').click(); state.details = true; active = true;
    }
    // The details-container success marker is set only after the upload phase ends.
    complete = downloadReady && cls('extra-details-container','succeeded') && cls('upload-value','succeeded');
    if (state.details && !complete) active = true;
    phase = !downloadReady ? 'Downloading' : cls('latency-container','succeeded') ? 'Uploading' : 'Measuring latency';
    fields.download = val('speed-value')+' '+val('speed-units');
    fields.upload = val('upload-value')+' '+val('upload-units');
    fields.ping = val('latency-value')+' '+val('latency-units');
    fields.loaded = val('bufferbloat-value')+' '+val('bufferbloat-units');
    fields.server = val('server-locations');
    if (cls('speed-value','failed') || cls('upload-value','failed')) phase = 'Provider reported an unstable or failed measurement';
  } else if (provider === 'Speedtest') {
    const result = document.querySelector('[data-testid="eot-results-card"],.result-container .result-data');
    const resultLink = Array.from(document.querySelectorAll('a[href]')).find(el => /speedtest\.net\/result\/\d+/.test(el.href));
    const hasResult = (visible(result) && /Download/i.test(text(result)) && /Upload/i.test(text(result))) || (!!resultLink && /Result ID:/i.test(body));
    const go = document.querySelector('button[aria-label^="start speed test"]') || exactButton(/^GO$/);
    if (!state.started && visible(go) && /^(GO|START)$/i.test(text(go))) clickStart(go);
    if (state.started && !hasResult) state.seenActive = true;
    complete = state.seenActive && hasResult;
    active = state.started && !complete;
    phase = active ? 'Measuring throughput and latency' : 'Selecting test server';
    fields.result_text = result ? text(result) : '';
    if (result) {
      for (const [name,label] of Object.entries({download:'Receiving Time',upload:'Sending Time'})) {
        let el=result.querySelector(`[aria-label="${label}"],[title="${label}"]`);
        for (let i=0;el && result.contains(el) && i<5;i++,el=el.parentElement) {
          const value=el.querySelector('h3');
          if (value) {fields[name]=text(value)+' '+(text(el).match(/\b(?:G|M|K)bps\b/i)?.[0]||'Mbps');break;}
        }
      }
      for (const [name,label] of Object.entries({ping:'Idle Latency',download_latency:'Download Latency',upload_latency:'Upload Latency'})) {
        const icon=result.querySelector(`[aria-label="${label}"],[title="${label}"]`);
        const value=icon?.parentElement?.querySelector('span');
        if (value && /^\d/.test(text(value))) fields[name]=text(value)+' ms';
      }
    }
    // Support the previous site's explicitly labeled result elements as well.
    for (const [name,selector] of Object.entries({download:'.download-speed',upload:'.upload-speed',ping:'.ping-speed',jitter:'.jitter-speed'})) {
      const el = document.querySelector(selector); if (el && visible(el)) fields[name] = text(el)+(name==='download'||name==='upload'?' Mbps':' ms');
    }
    if (resultLink) fields.result_url = resultLink.href;
    // The full-width and mini-test layouts share these visible English labels,
    // but the full-width layout does not expose an eot-results-card wrapper.
    if (complete) {
      for (const direction of ['download','upload']) {
        const match=body.match(new RegExp(`\\b${direction}\\s+(Gbps|Mbps|Kbps)\\s+([0-9][0-9,.]*)`,'i'));
        if (match) fields[direction]=match[2]+' '+match[1];
      }
      const pings=body.match(/\bPing\s+ms\s+([0-9]+(?:\.[0-9]+)?)(?:\s+([0-9]+(?:\.[0-9]+)?))?(?:\s+([0-9]+(?:\.[0-9]+)?))?/i);
      if (pings) for (const [i,key] of ['ping','download_latency','upload_latency'].entries()) {
        if (pings[i+1]) fields[key]=pings[i+1]+' ms';
      }
      if (!fields.result_text) fields.result_text=body.match(/DOWNLOAD\s+(?:Gbps|Mbps|Kbps)[\s\S]*?(?=Connections|Desktop Apps|$)/i)?.[0] || '';
    }
  } else if (provider === 'Devina') {
    const run = exactButton(/^Run speed test$/i);
    const running = /Measuring your network speed/.test(body);
    if (!state.started && run) clickStart(run);
    if (running) {state.started=true;state.seenActive=true;}
    complete = state.seenActive && /Your latency (?:did not increase|increased)/.test(body) && /Under ideal conditions/.test(body) && !running;
    active = !complete && (active || running || (state.started && !run));
    phase = 'Measuring idle, download-loaded and upload-loaded latency';
    // Read the small rendered metric cards, excluding the explanatory page text.
    for (const label of ['Latency','Download','Upload']) {
      const heading = Array.from(document.querySelectorAll('p,span')).find(el => visible(el) && text(el)===label);
      let el = heading;
      for (let i=0; el && i<7; i++,el=el.parentElement) {
        const content = text(el);
        if (content.startsWith(label) && /\d[\s\S]*\bms\b/.test(content) && (label==='Latency' || /[KMGT]?b(?:it\/s|ps)|[KMGT]b\/s/i.test(content))) {
          fields[label.toLowerCase()] = content; break;
        }
      }
    }
    fields.assessment = body.match(/Your latency (?:did not increase|increased)[^\n]*/)?.[0] || '';
  }
  // Freshness is confirmed in Rust: activity first, then two matching complete snapshots.
  return {origin:location.origin,text:body.slice(0,32768),active,complete,phase,fields};
})()
