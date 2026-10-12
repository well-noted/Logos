// The launcher and first launch (design 2.1, 3.1): find NetLogo, check the engine, open
// something. Logos doesn't download or install NetLogo (D-31).

import { open as openUrl } from "./links";
import { Icon } from "./icons";
import { ipc } from "./ipc";
import {
  appInfo, chooseNetLogoFolder, engine, openModel, openModelDialog, settings, settingsOpen,
} from "./state";

export function Launcher() {
  const e = engine.value;
  const recent = settings.value.recent;
  const ready = e.state === "ready";
  return (
    <main class="launcher" aria-label="Start">
      <div class="launcher-inner">
        <h1>Logos<small>NetLogo models, from teaching exercises to research studies.</small></h1>

        <section class="card" aria-labelledby="engine-h">
          <h2 id="engine-h">NetLogo</h2>
          <ol class="steps">
            <li>
              <div>
                <strong>Find NetLogo</strong>
                {e.state === "searching" && <p class="muted">Looking for NetLogo {appInfo.value?.netlogoVersion ?? "7.0.4"}…</p>}
                {e.installs.length > 0 && e.installs.map((i) => (
                  <div class={`install${i.path === e.home ? " chosen" : ""}`}>
                    {i.path === e.home ? <Icon name="check" title="In use" /> : <span style="width:16px" />}
                    <span>
                      <div>NetLogo {i.version} {i.version !== "7.0.4" && <span class="muted">(Logos needs 7.0.4)</span>}</div>
                      <div class="path">{i.path}</div>
                    </span>
                  </div>
                ))}
                {e.state === "notFound" && (
                  <div>
                    <p>Logos runs models on NetLogo itself, so it needs NetLogo 7.0.4 installed on this computer. {e.installs.length > 0 ? "The installs found are other versions." : "None was found in the usual places."}</p>
                    <p>Install NetLogo 7.0.4 from NetLogo's download page, then come back, or choose the folder if it is installed somewhere else.</p>
                    <div class="actions">
                      <button class="btn" onClick={() => openUrl("https://www.netlogo.org/downloads/")}>NetLogo's download page</button>
                    </div>
                  </div>
                )}
                <div class="actions" style="margin-top: 8px">
                  <button class="btn small" onClick={chooseNetLogoFolder}>Choose folder…</button>
                  <button class="btn small" onClick={() => ipc.engineRestart().then((s) => { engine.value = s; })}>Look again</button>
                </div>
              </div>
            </li>
            <li>
              <div>
                <strong>Check the engine</strong>
                {e.state === "starting" && <p class="muted">Starting the bridge…</p>}
                {ready && e.hello && (
                  <dl class="kv" style="margin-top: 6px">
                    <dt>Engine</dt><dd class="ok">{e.hello.netlogoVersion ?? "NetLogo"} ready</dd>
                    <dt>Java</dt><dd>{e.hello.javaVersion} ({e.hello.javaVendor})</dd>
                    <dt>Bridge</dt><dd>{e.hello.bridgeVersion}</dd>
                    <dt>Extensions</dt><dd>{e.hello.extensionsDirs.join("; ")}</dd>
                  </dl>
                )}
                {e.state === "failed" && (
                  <div>
                    <p class="err">The bridge didn't start: {e.error}</p>
                    {e.home && <p class="muted">NetLogo folder tried: <span class="mono">{e.home}</span></p>}
                    {e.log && <pre class="log" aria-label="Bridge log">{e.log}</pre>}
                  </div>
                )}
                {!ready && e.state !== "starting" && e.state !== "failed" && <p class="muted">Waiting for NetLogo.</p>}
              </div>
            </li>
            <li>
              <div>
                <strong>Open something</strong>
                <div class="actions" style="margin-top: 8px">
                  <button class="btn primary" disabled={!ready} onClick={() => openModelDialog()}>Open a model…</button>
                  <button class="btn" disabled={!ready || !e.hello?.modelsDir} onClick={() => openModelDialog(e.hello?.modelsDir ?? undefined)}>Models Library…</button>
                </div>
                <p class="muted" style="font-size: 12px">Older .nlogo models are converted to a .nlogox copy through NetLogo; the original stays unchanged.</p>
              </div>
            </li>
          </ol>
        </section>

        <section class="card" aria-labelledby="recent-h">
          <h2 id="recent-h">Recent models</h2>
          {recent.length === 0 ? <p class="muted">Models you open appear here.</p> : (
            <ul class="recent">
              {recent.map((p) => {
                const name = p.split(/[\\/]/).pop();
                return (
                  <li><button disabled={!ready} onClick={() => openModel(p)}>
                    <div>{name}</div><div class="p">{p}</div>
                  </button></li>
                );
              })}
            </ul>
          )}
          <h2 style="margin-top: 18px">Setup</h2>
          <div class="actions">
            <button class="btn" onClick={() => { settingsOpen.value = true; }}><Icon name="gear" /> Theme and font size…</button>
          </div>
          <p class="muted" style="font-size: 12px; margin-top: 12px">
            Logos {appInfo.value?.version} · settings in <span class="mono">{appInfo.value?.dataDir}</span>
          </p>
        </section>
      </div>
    </main>
  );
}
