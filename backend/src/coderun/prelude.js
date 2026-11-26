// What a run_code script finds before its own code runs (coderun/script.rs): `tools`, `skills`,
// `args` and `console`, built on the host's four functions, which this takes and removes from the
// global scope. It gives the host, and only the host, `settle`, which the host calls with each
// tool call's answer, and `run`, which runs the script's async function and hands the host what it
// returned or threw: neither is on the global scope, so the script can't replace them.
(() => {
  const host = { call: __janusCall, log: __janusLog, ret: __janusReturn, threw: __janusThrow };
  const limit = __janusLimit;
  const names = JSON.parse(__janusToolNames);
  const srcs = JSON.parse(__janusSkills);
  globalThis.args = JSON.parse(__janusArgs);
  for (const k of ["__janusCall", "__janusLog", "__janusReturn", "__janusThrow", "__janusLimit", "__janusToolNames", "__janusSkills", "__janusArgs"]) {
    delete globalThis[k];
  }

  // An Error by its name, message and where it was thrown; anything else as `show` shows it.
  const errorText = (e) => {
    if (e === undefined) return "undefined";
    if (e instanceof Error) {
      const head = (e.name ? e.name + ": " : "") + e.message;
      const stack = typeof e.stack === "string" ? e.stack.trim() : "";
      return stack ? head + "\n" + stack : head;
    }
    return show(e);
  };
  // A value as console.log shows it: a string as it is, an Error by errorText, anything else as
  // JSON when it has some.
  const show = (v) => {
    if (v === undefined) return "undefined";
    if (typeof v === "string") return v;
    if (v instanceof Error) return errorText(v);
    try {
      const s = JSON.stringify(v);
      if (s !== undefined) return s;
    } catch (_) {}
    return String(v);
  };

  const console = {};
  for (const k of ["log", "info", "warn", "error", "debug"]) {
    console[k] = (...a) => host.log(a.map(show).join(" "));
  }
  globalThis.console = console;

  // Each tool is an async function of its arguments; the host runs the call and answers it
  // through __janusSettle.
  const pending = new Map();
  let calls = 0;
  const tool = (name) => (a) =>
    new Promise((resolve, reject) => {
      if (++calls > limit) {
        reject(new Error("more than " + limit + " tool calls in one script"));
        return;
      }
      const json = a === undefined || a === null ? "{}" : JSON.stringify(a);
      pending.set(calls, { resolve, reject });
      host.call(calls, String(name), json);
    });
  const own = {};
  for (const n of names) {
    const f = tool(n);
    own[n] = f;
    const alt = n.replaceAll("-", "_");
    if (alt !== n && !(alt in own)) own[alt] = f;
  }
  // A name no tool listed is still asked for: a server's list can change after it was listed.
  globalThis.tools = new Proxy(own, {
    get(o, k) {
      if (typeof k !== "string" || k in o || k === "then" || k === "toJSON") return o[k];
      return tool(k);
    },
  });

  // A skill's code.js runs the first time it is called, by an indirect eval: in the global scope,
  // with tools and skills.
  const made = {};
  const skills = {};
  for (const n of Object.keys(srcs)) {
    const f = async (a) => {
      if (!(n in made)) made[n] = (0, eval)(srcs[n]);
      if (typeof made[n] !== "function") throw new Error("the skill " + n + "'s code.js doesn't export a function by default");
      return await made[n](a === undefined || a === null ? {} : a);
    };
    skills[n] = f;
    const alt = n.replaceAll("-", "_");
    if (alt !== n && !(alt in skills)) skills[alt] = f;
  }
  globalThis.skills = new Proxy(skills, {
    get(o, k) {
      if (typeof k !== "string" || k in o || k === "then" || k === "toJSON") return o[k];
      return async () => {
        throw new Error("no skill with code is called " + k);
      };
    },
  });

  // A tool's answer as the script is given it: its structuredContent, else its text (parsed when
  // it is JSON; texts joined by a line), else its content as it is, for an image. An error result
  // is thrown with its text.
  const settle = (id, res, err) => {
    const p = pending.get(id);
    if (!p) return;
    pending.delete(id);
    if (err !== undefined) {
      p.reject(new Error(err));
      return;
    }
    let r;
    try {
      r = JSON.parse(res);
    } catch (e) {
      p.reject(e);
      return;
    }
    const content = r && Array.isArray(r.content) ? r.content : [];
    const texts = [];
    let other = false;
    for (const c of content) {
      if (c && c.type === "text") texts.push(String(c.text ?? ""));
      else other = true;
    }
    const text = texts.join("\n");
    if (r && r.isError) {
      p.reject(new Error(text || "the tool failed"));
      return;
    }
    if (r && r.structuredContent !== undefined && r.structuredContent !== null) {
      p.resolve(r.structuredContent);
      return;
    }
    if (other) {
      p.resolve(content);
      return;
    }
    const t = text.trim();
    if (t.startsWith("{") || t.startsWith("[")) {
      try {
        p.resolve(JSON.parse(t));
        return;
      } catch (_) {}
    }
    p.resolve(text);
  };

  const run = (f) => {
    f().then(
      (v) => {
        if (v === undefined) host.ret("none", "");
        else if (typeof v === "string") host.ret("string", v);
        else host.ret("shown", show(v));
      },
      (e) => host.threw(errorText(e)),
    );
  };
  return { settle, run };
})();
