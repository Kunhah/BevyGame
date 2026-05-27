const STAT_OPTIONS = ["Mind", "Agility", "Strength", "Morale", "Lethality"];
const DAMAGE_TYPE_OPTIONS = ["Physical", "Fire", "Ice", "Lightning"];

const SPEAKER_SLOTS = [
  "slot1", "slot2", "slot3", "slot4", "slot5", "slot6",
  "slot7", "slot8", "slot9", "slot10", "slot11", "slot12",
];

const NODE_KINDS = ["line", "choice", "scene"];
const CONDITION_KINDS = [
  "flag", "not_flag", "has_item", "quest_status",
  "reputation_at_least", "all", "any", "not",
];
const EFFECT_KINDS = [
  "set_flag", "clear_flag", "reputation",
  "give_item", "take_item", "give_coin", "take_coin",
  "start_quest", "advance_objective", "accept_contract",
  "play_sfx", "play_music",
  "spawn_interactable", "despawn_interactable", "change_scene",
];
const SCENE_ACTION_KINDS = [
  "enter_character", "exit_character", "set_expression",
  "set_background", "play_music", "play_sfx",
  "wait", "fade_out", "fade_in", "shake_screen",
];
const QUEST_STATUS_FILTERS = ["inactive", "active", "completed", "failed"];
const REPUTATION_TARGET_KINDS = [
  "local_governor", "local_merchant", "local_clan",
  "governor", "merchant", "clan",
];

// All RON parsing/serialization is handled by the Rust editor server
// (see src/bin/editor_server.rs). The JS UI only speaks JSON to the API.
const API = {
  abilities: "/api/abilities",
  scenes: "/api/scenes",
};

const state = {
  editor: "abilities",
  abilities: [],
  scenes: [],
  selectedAbilityIndex: null,
  selectedSceneIndex: null,
  selectedNodeId: null,
  search: "",
  dirty: { abilities: false, scenes: false },
  loaded: { abilities: false, scenes: false },
  messages: [],
  validationMessages: { abilities: [], scenes: [] },
};

const elements = {
  modeButtons: [...document.querySelectorAll(".mode-button")],
  listTitle: document.querySelector("#list-title"),
  newEntryButton: document.querySelector("#new-entry-button"),
  searchInput: document.querySelector("#search-input"),
  entryList: document.querySelector("#entry-list"),
  editorRoot: document.querySelector("#editor-root"),
  fileLabel: document.querySelector("#file-label"),
  dirtyLabel: document.querySelector("#dirty-label"),
  totalCount: document.querySelector("#total-count"),
  selectedLabel: document.querySelector("#selected-label"),
  validationCount: document.querySelector("#validation-count"),
  messages: document.querySelector("#messages"),
  reloadButton: document.querySelector("#reload-button"),
  saveButton: document.querySelector("#save-button"),
  downloadButton: document.querySelector("#download-button"),
  validateButton: document.querySelector("#validate-button"),
};

// ---------------------------------------------------------------------------
// Generic utilities
// ---------------------------------------------------------------------------

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

function currentEntries() {
  return state.editor === "abilities" ? state.abilities : state.scenes;
}

function currentSelectionIndex() {
  return state.editor === "abilities" ? state.selectedAbilityIndex : state.selectedSceneIndex;
}

function setSelectionIndex(index) {
  if (state.editor === "abilities") {
    state.selectedAbilityIndex = index;
  } else {
    state.selectedSceneIndex = index;
    state.selectedNodeId = null;
  }
}

function setDirty(value) {
  state.dirty[state.editor] = value;
}

function activeValidationMessages() {
  return state.validationMessages[state.editor];
}

function pushMessage(kind, text) {
  state.messages.unshift({
    id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
    kind,
    text,
  });
  state.messages = state.messages.slice(0, 20);
  renderMessages();
}

function apiUrl() {
  return state.editor === "abilities" ? API.abilities : API.scenes;
}

function defaultDownloadName() {
  return state.editor === "abilities" ? "abilities.ron" : "scenes.json";
}

// ---------------------------------------------------------------------------
// Defaults / id generators
// ---------------------------------------------------------------------------

function createDefaultAbility() {
  return {
    id: nextFreeAbilityId(state.abilities, 0),
    next_id: null,
    name: "New Ability",
    health_cost: 0,
    magic_cost: 0,
    magic_school: "Kiho",
    action_point_cost: 0,
    cooldown: 0,
    description: "",
    effects: [],
    shape: "Select",
    duration: 0,
    targets: 1,
  };
}

function nextFreeAbilityId(abilities, level) {
  const used = new Set(
    abilities
      .filter((ability) => ((ability.id >> 8) & 0xff) === level)
      .map((ability) => ability.id & 0xff),
  );
  for (let i = 0; i <= 0xff; i += 1) {
    if (!used.has(i)) {
      return ((level & 0xff) << 8) | i;
    }
  }
  return 0;
}

function createDefaultScene() {
  const id = uniqueSceneId(state.scenes);
  const startNodeId = "node_1";
  return {
    id,
    background: null,
    music: null,
    start: startNodeId,
    nodes: {
      [startNodeId]: createDefaultLineNode(),
    },
  };
}

function createDefaultLineNode() {
  return {
    line: {
      speaker: { name: "", slot: "slot1", expression: null },
      text: "",
      on_enter: [],
      condition: null,
      next: null,
    },
  };
}

function createDefaultChoiceNode() {
  return {
    choice: {
      prompt: { name: "", slot: "slot1", expression: null },
      prompt_text: "",
      options: [createDefaultChoiceOption()],
    },
  };
}

function createDefaultSceneNode() {
  return {
    scene: {
      actions: [],
      next: null,
    },
  };
}

function createDefaultChoiceOption() {
  return {
    text: "",
    condition: null,
    effects: [],
    next: null,
    legacy_event_id: 0,
  };
}

function createNodeOfKind(kind) {
  if (kind === "line") return createDefaultLineNode();
  if (kind === "choice") return createDefaultChoiceNode();
  return createDefaultSceneNode();
}

function uniqueSceneId(scenes) {
  const ids = new Set(scenes.map((scene) => scene.id));
  let n = scenes.length + 1;
  while (ids.has(`scene_${n}`)) {
    n += 1;
  }
  return `scene_${n}`;
}

function uniqueNodeId(scene, prefix = "node") {
  const ids = new Set(Object.keys(scene.nodes ?? {}));
  let n = ids.size + 1;
  while (ids.has(`${prefix}_${n}`)) {
    n += 1;
  }
  return `${prefix}_${n}`;
}

// ---------------------------------------------------------------------------
// Variant helpers (server uses externally-tagged enums for Rust enums)
// ---------------------------------------------------------------------------

function variantTag(value, fallback) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object") {
    const keys = Object.keys(value);
    if (keys.length > 0) return keys[0];
  }
  return fallback;
}

function abilityShapeKind(shape) {
  return variantTag(shape, "Select");
}

function effectKind(effect) {
  return variantTag(effect, "Heal");
}

function nodeKind(node) {
  return variantTag(node, "line");
}

function conditionKind(cond) {
  return variantTag(cond, "flag");
}

function dialogueEffectKind(effect) {
  return variantTag(effect, "set_flag");
}

function sceneActionKind(action) {
  return variantTag(action, "wait");
}

function reputationTargetKind(target) {
  return variantTag(target, "local_governor");
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

function validateAbilities(abilities) {
  const messages = [];
  const ids = new Set();
  for (const ability of abilities) {
    if (ids.has(ability.id)) messages.push(`Duplicate ability id: ${ability.id}`);
    ids.add(ability.id);
  }
  for (const ability of abilities) {
    if (ability.next_id != null && !ids.has(ability.next_id)) {
      messages.push(`Ability ${ability.id} has invalid next_id ${ability.next_id}`);
    }
    for (const effect of ability.effects ?? []) {
      if (effect?.Buff?.effects) {
        for (const linkedId of effect.Buff.effects) {
          if (!ids.has(linkedId)) {
            messages.push(
              `Ability ${ability.id} has Buff effect referencing missing ability id ${linkedId}`,
            );
          }
        }
      }
    }
  }
  return messages.length > 0 ? messages : ["No issues detected."];
}

function validateScenes(scenes) {
  const messages = [];
  const ids = new Set();
  for (const scene of scenes) {
    if (ids.has(scene.id)) messages.push(`Duplicate scene id: ${scene.id}`);
    ids.add(scene.id);
    if (!/^[a-zA-Z0-9_-]+$/.test(scene.id)) {
      messages.push(`Scene "${scene.id}" has invalid id (use [a-zA-Z0-9_-])`);
    }
    const nodeIds = new Set(Object.keys(scene.nodes ?? {}));
    if (!nodeIds.has(scene.start)) {
      messages.push(`Scene "${scene.id}" start "${scene.start}" not in nodes`);
    }
    for (const [id, node] of Object.entries(scene.nodes ?? {})) {
      const kind = nodeKind(node);
      const data = node[kind];
      const refs = [];
      if (kind === "line" || kind === "scene") {
        if (data.next) refs.push(data.next);
      }
      if (kind === "choice") {
        for (const opt of data.options ?? []) {
          if (opt.next) refs.push(opt.next);
        }
      }
      for (const ref of refs) {
        if (!nodeIds.has(ref)) {
          messages.push(`Scene "${scene.id}" node "${id}" → unknown next "${ref}"`);
        }
      }
    }
  }
  return messages.length > 0 ? messages : ["No issues detected."];
}

// ---------------------------------------------------------------------------
// Rendering: messages + summary + list
// ---------------------------------------------------------------------------

function renderMessages() {
  elements.messages.innerHTML = "";
  for (const message of state.messages) {
    const node = document.createElement("div");
    node.className = `message ${message.kind}`;
    node.textContent = message.text;
    elements.messages.appendChild(node);
  }
}

function renderList() {
  const entries = currentEntries();
  const selectedIndex = currentSelectionIndex();
  const search = state.search.trim().toLowerCase();
  elements.entryList.innerHTML = "";

  const filteredEntries = entries
    .map((entry, index) => ({ entry, index }))
    .filter(({ entry }) => {
      if (!search) return true;
      const haystack =
        state.editor === "abilities"
          ? `${entry.id} ${entry.name}`
          : `${entry.id}`;
      return haystack.toLowerCase().includes(search);
    });

  if (filteredEntries.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = entries.length === 0 ? "Nothing here yet. Click New." : "Nothing matches.";
    elements.entryList.appendChild(empty);
    return;
  }

  for (const { entry, index } of filteredEntries) {
    const row = document.createElement("button");
    row.className = "entry-row";
    if (index === selectedIndex) row.classList.add("active");
    row.addEventListener("click", () => {
      setSelectionIndex(index);
      render();
    });

    const header = document.createElement("div");
    header.className = "entry-header";

    const title = document.createElement("div");
    title.className = "entry-title";
    title.textContent =
      state.editor === "abilities" ? entry.name || `Ability ${entry.id}` : entry.id;

    const token = document.createElement("span");
    token.className = "entry-token";
    token.textContent =
      state.editor === "abilities"
        ? `#${entry.id}`
        : `${Object.keys(entry.nodes ?? {}).length} nodes`;

    header.append(title, token);

    const subtitle = document.createElement("div");
    subtitle.className = "entry-subtitle";
    subtitle.textContent =
      state.editor === "abilities"
        ? `${entry.health_cost} HP · ${entry.magic_cost} MP · ${entry.action_point_cost} AP`
        : `start: ${entry.start ?? "—"}`;

    row.append(header, subtitle);
    elements.entryList.appendChild(row);
  }
}

function renderSummary() {
  const entries = currentEntries();
  const selectedIndex = currentSelectionIndex();
  const selected = selectedIndex === null ? null : entries[selectedIndex];

  elements.totalCount.textContent = String(entries.length);
  elements.selectedLabel.textContent =
    selected
      ? state.editor === "abilities"
        ? selected.name || `Ability ${selected.id}`
        : selected.id
      : "None";

  elements.dirtyLabel.textContent = state.dirty[state.editor] ? "Unsaved changes" : "Up to date";
  elements.dirtyLabel.classList.toggle("muted", !state.dirty[state.editor]);
  elements.fileLabel.textContent =
    state.editor === "abilities" ? "Backed by Rust editor server" : "assets/data/dialogues/";

  elements.listTitle.textContent = state.editor === "abilities" ? "Abilities" : "Scenes";

  const validationMessages = activeValidationMessages();
  const issueCount =
    validationMessages.length === 0 || validationMessages[0] === "No issues detected."
      ? 0
      : validationMessages.length;
  elements.validationCount.textContent = `${issueCount} issue${issueCount === 1 ? "" : "s"}`;

  for (const button of elements.modeButtons) {
    button.classList.toggle("active", button.dataset.editor === state.editor);
  }
}

// ---------------------------------------------------------------------------
// Form primitives
// ---------------------------------------------------------------------------

function numberInput(value, onChange, options = {}) {
  const input = document.createElement("input");
  input.type = "number";
  input.value = value ?? 0;
  if (options.step !== undefined) input.step = String(options.step);
  if (options.min !== undefined) input.min = String(options.min);
  input.addEventListener("input", () => onChange(Number(input.value)));
  return input;
}

function textInput(value, onChange) {
  const input = document.createElement("input");
  input.type = "text";
  input.value = value ?? "";
  input.addEventListener("input", () => onChange(input.value));
  return input;
}

function textArea(value, onChange) {
  const input = document.createElement("textarea");
  input.value = value ?? "";
  input.addEventListener("input", () => onChange(input.value));
  return input;
}

function selectInput(options, value, onChange) {
  const select = document.createElement("select");
  for (const optionValue of options) {
    const option = document.createElement("option");
    option.value = optionValue;
    option.textContent = optionValue;
    option.selected = optionValue === value;
    select.appendChild(option);
  }
  select.addEventListener("change", () => onChange(select.value));
  return select;
}

function checkboxInput(value, onChange, labelText) {
  const wrapper = document.createElement("label");
  wrapper.className = "stacked-field";
  const row = document.createElement("div");
  row.className = "inline-fields";
  row.style.alignItems = "center";
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.checked = Boolean(value);
  checkbox.style.width = "auto";
  checkbox.addEventListener("change", () => onChange(checkbox.checked));
  const text = document.createElement("span");
  text.textContent = labelText;
  row.append(checkbox, text);
  wrapper.appendChild(row);
  return wrapper;
}

function field(label, control) {
  const wrapper = document.createElement("label");
  wrapper.className = "stacked-field";
  const text = document.createElement("span");
  text.textContent = label;
  wrapper.append(text, control);
  return wrapper;
}

// ---------------------------------------------------------------------------
// Mutation helpers
// ---------------------------------------------------------------------------

function replaceSelectedAbility(mutator) {
  if (state.selectedAbilityIndex === null) return;
  const current = state.abilities[state.selectedAbilityIndex];
  if (!current) return;
  const next = clone(current);
  mutator(next);
  state.abilities[state.selectedAbilityIndex] = next;
  setDirty(true);
  render();
}

function replaceSelectedScene(mutator) {
  if (state.selectedSceneIndex === null) return;
  const current = state.scenes[state.selectedSceneIndex];
  if (!current) return;
  const next = clone(current);
  mutator(next);
  state.scenes[state.selectedSceneIndex] = next;
  setDirty(true);
  render();
}

function replaceSelectedNode(mutator) {
  replaceSelectedScene((scene) => {
    if (!state.selectedNodeId) return;
    const node = scene.nodes[state.selectedNodeId];
    if (!node) return;
    mutator(node);
  });
}

// ---------------------------------------------------------------------------
// Ability shape / effect defaults (unchanged)
// ---------------------------------------------------------------------------

function shapeForKind(kind) {
  if (kind === "Radius") return { Radius: 1 };
  if (kind === "Line") return { Line: { length: 1, thickness: 1 } };
  if (kind === "Cone") return { Cone: { angle: 45, radius: 2 } };
  return "Select";
}

function effectForKind(kind) {
  if (kind === "Damage") {
    return {
      Damage: {
        floor: 1,
        ceiling: 2,
        damage_type: "Physical",
        scaled_with: "Mind",
        defended_with: "Mind",
      },
    };
  }
  if (kind === "Buff") {
    return {
      Buff: {
        stat: "Morale",
        multiplier: 1,
        effects: null,
        scaled_with: "Mind",
      },
    };
  }
  return { Heal: { floor: 1, ceiling: 2, scaled_with: "Mind" } };
}

// ---------------------------------------------------------------------------
// Dialogue scene editor
// ---------------------------------------------------------------------------

function buildSceneEditor() {
  const container = document.createElement("section");
  container.className = "card section-stack";
  const selected =
    state.selectedSceneIndex === null ? null : state.scenes[state.selectedSceneIndex];

  if (!selected) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = "Select a scene to edit or create a new one.";
    container.appendChild(empty);
    return container;
  }

  container.append(
    buildSceneHeader(selected),
    buildSceneProperties(selected),
    buildNodeListPanel(selected),
    buildNodeEditorPanel(selected),
  );
  return container;
}

function buildSceneHeader(scene) {
  const header = document.createElement("div");
  header.className = "panel-header";
  header.innerHTML = `<div><p class="panel-kicker">Scene</p><h2>${scene.id}</h2></div>`;

  const actions = document.createElement("div");
  actions.className = "action-cluster";

  const duplicateButton = document.createElement("button");
  duplicateButton.textContent = "Duplicate";
  duplicateButton.addEventListener("click", () => {
    const copy = clone(scene);
    copy.id = uniqueSceneId(state.scenes);
    state.scenes.push(copy);
    state.selectedSceneIndex = state.scenes.length - 1;
    state.selectedNodeId = null;
    setDirty(true);
    render();
  });

  const deleteButton = document.createElement("button");
  deleteButton.textContent = "Delete";
  deleteButton.className = "danger-button";
  deleteButton.addEventListener("click", () => {
    state.scenes.splice(state.selectedSceneIndex, 1);
    state.selectedSceneIndex =
      state.scenes.length === 0
        ? null
        : Math.min(state.selectedSceneIndex, state.scenes.length - 1);
    state.selectedNodeId = null;
    setDirty(true);
    render();
  });

  actions.append(duplicateButton, deleteButton);
  header.appendChild(actions);
  return header;
}

function buildSceneProperties(scene) {
  const card = document.createElement("div");
  card.className = "editor-card section-stack";
  card.innerHTML = `<p class="panel-kicker">Properties</p>`;

  const idGrid = document.createElement("div");
  idGrid.className = "field-grid two";
  idGrid.append(
    field(
      "Scene ID",
      textInput(scene.id, (value) =>
        replaceSelectedScene((s) => {
          s.id = value;
        }),
      ),
    ),
    field(
      "Start node",
      selectInput(
        Object.keys(scene.nodes ?? {}),
        scene.start ?? "",
        (value) =>
          replaceSelectedScene((s) => {
            s.start = value;
          }),
      ),
    ),
  );

  const assetGrid = document.createElement("div");
  assetGrid.className = "field-grid two";
  assetGrid.append(
    field(
      "Background (asset path)",
      textInput(scene.background ?? "", (value) =>
        replaceSelectedScene((s) => {
          s.background = value === "" ? null : value;
        }),
      ),
    ),
    field(
      "Music (asset name)",
      textInput(scene.music ?? "", (value) =>
        replaceSelectedScene((s) => {
          s.music = value === "" ? null : value;
        }),
      ),
    ),
  );

  card.append(idGrid, assetGrid);
  return card;
}

function buildNodeListPanel(scene) {
  const card = document.createElement("div");
  card.className = "editor-card section-stack";

  const header = document.createElement("div");
  header.className = "choice-header";
  const nodeCount = Object.keys(scene.nodes ?? {}).length;
  header.innerHTML = `<div><p class="panel-kicker">Nodes (${nodeCount})</p><h2>Graph</h2></div>`;

  const addCluster = document.createElement("div");
  addCluster.className = "action-cluster";
  for (const kind of NODE_KINDS) {
    const button = document.createElement("button");
    button.textContent = `+ ${kind[0].toUpperCase()}${kind.slice(1)}`;
    button.addEventListener("click", () => {
      const newId = uniqueNodeId(scene, kind);
      replaceSelectedScene((s) => {
        s.nodes[newId] = createNodeOfKind(kind);
        if (Object.keys(s.nodes).length === 1) {
          s.start = newId;
        }
      });
      state.selectedNodeId = newId;
      render();
    });
    addCluster.appendChild(button);
  }
  header.appendChild(addCluster);
  card.appendChild(header);

  const list = document.createElement("div");
  list.className = "node-list";
  const entries = Object.entries(scene.nodes ?? {});
  if (entries.length === 0) {
    const empty = document.createElement("div");
    empty.className = "help-text";
    empty.textContent = "No nodes yet — add Line/Choice/Scene to begin.";
    card.appendChild(empty);
    return card;
  }

  for (const [nodeId, node] of entries) {
    const row = document.createElement("button");
    row.className = "node-row";
    if (nodeId === state.selectedNodeId) row.classList.add("active");
    if (nodeId === scene.start) row.classList.add("start");
    row.addEventListener("click", () => {
      state.selectedNodeId = nodeId;
      render();
    });
    const head = document.createElement("div");
    head.className = "entry-header";
    const title = document.createElement("span");
    title.className = "entry-title";
    title.textContent = nodeId;
    const tag = document.createElement("span");
    tag.className = `node-tag ${nodeKind(node)}`;
    tag.textContent = nodeKind(node);
    head.append(title, tag);

    const subtitle = document.createElement("div");
    subtitle.className = "entry-subtitle";
    subtitle.textContent = nodeSubtitle(node);

    row.append(head, subtitle);
    list.appendChild(row);
  }
  card.appendChild(list);
  return card;
}

function nodeSubtitle(node) {
  const kind = nodeKind(node);
  const data = node[kind] ?? {};
  if (kind === "line") {
    const speaker = data.speaker?.name || "(narration)";
    const preview = (data.text ?? "").slice(0, 60);
    return `${speaker}: ${preview}${(data.text ?? "").length > 60 ? "…" : ""}`;
  }
  if (kind === "choice") {
    return `${(data.options ?? []).length} option(s)`;
  }
  return `${(data.actions ?? []).length} action(s)`;
}

function buildNodeEditorPanel(scene) {
  const card = document.createElement("div");
  card.className = "editor-card section-stack";

  if (!state.selectedNodeId || !scene.nodes[state.selectedNodeId]) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = "Select a node above to edit.";
    card.appendChild(empty);
    return card;
  }

  const nodeId = state.selectedNodeId;
  const node = scene.nodes[nodeId];
  const kind = nodeKind(node);

  const header = document.createElement("div");
  header.className = "panel-header";
  header.innerHTML = `<div><p class="panel-kicker">Node (${kind})</p><h2>${nodeId}</h2></div>`;

  const headerActions = document.createElement("div");
  headerActions.className = "action-cluster";

  const renameButton = document.createElement("button");
  renameButton.textContent = "Rename";
  renameButton.addEventListener("click", () => {
    const next = prompt("New node id", nodeId);
    if (!next || next === nodeId) return;
    if (Object.keys(scene.nodes).includes(next)) {
      pushMessage("error", `Node id "${next}" already exists`);
      return;
    }
    replaceSelectedScene((s) => {
      s.nodes[next] = s.nodes[nodeId];
      delete s.nodes[nodeId];
      if (s.start === nodeId) s.start = next;
      for (const other of Object.values(s.nodes)) {
        const otherKind = nodeKind(other);
        const otherData = other[otherKind];
        if (otherKind === "line" || otherKind === "scene") {
          if (otherData.next === nodeId) otherData.next = next;
        }
        if (otherKind === "choice") {
          for (const opt of otherData.options ?? []) {
            if (opt.next === nodeId) opt.next = next;
          }
        }
      }
    });
    state.selectedNodeId = next;
    render();
  });

  const kindSelect = selectInput(NODE_KINDS, kind, (value) => {
    if (value === kind) return;
    if (!confirm(`Convert node "${nodeId}" to ${value}? Existing fields will be lost.`)) return;
    replaceSelectedScene((s) => {
      s.nodes[nodeId] = createNodeOfKind(value);
    });
  });

  const deleteButton = document.createElement("button");
  deleteButton.textContent = "Delete";
  deleteButton.className = "danger-button";
  deleteButton.addEventListener("click", () => {
    if (!confirm(`Delete node "${nodeId}"?`)) return;
    replaceSelectedScene((s) => {
      delete s.nodes[nodeId];
      if (s.start === nodeId) {
        s.start = Object.keys(s.nodes)[0] ?? "";
      }
    });
    state.selectedNodeId = null;
    render();
  });

  headerActions.append(renameButton, kindSelect, deleteButton);
  header.appendChild(headerActions);
  card.appendChild(header);

  if (kind === "line") {
    card.appendChild(buildLineNodeBody(scene, node.line));
  } else if (kind === "choice") {
    card.appendChild(buildChoiceNodeBody(scene, node.choice));
  } else {
    card.appendChild(buildSceneNodeBody(scene, node.scene));
  }

  return card;
}

function buildLineNodeBody(scene, data) {
  const root = document.createElement("div");
  root.className = "section-stack";

  root.appendChild(
    buildSpeakerEditor("Speaker", data.speaker, (mutator) =>
      replaceSelectedNode((node) => {
        if (!node.line.speaker) {
          node.line.speaker = { name: "", slot: "slot1", expression: null };
        }
        mutator(node.line.speaker);
      }),
    ),
  );

  root.appendChild(
    field(
      "Text",
      textArea(data.text, (value) =>
        replaceSelectedNode((node) => {
          node.line.text = value;
        }),
      ),
    ),
  );

  root.appendChild(
    field(
      "Next",
      buildNodeRefSelect(scene, data.next, (value) =>
        replaceSelectedNode((node) => {
          node.line.next = value;
        }),
      ),
    ),
  );

  root.appendChild(
    buildConditionEditor("Condition", data.condition, (value) =>
      replaceSelectedNode((node) => {
        node.line.condition = value;
      }),
    ),
  );

  root.appendChild(
    buildEffectListEditor("On enter effects", data.on_enter ?? [], (next) =>
      replaceSelectedNode((node) => {
        node.line.on_enter = next;
      }),
    ),
  );

  return root;
}

function buildChoiceNodeBody(scene, data) {
  const root = document.createElement("div");
  root.className = "section-stack";

  if (data.prompt) {
    root.appendChild(
      buildSpeakerEditor("Prompt speaker", data.prompt, (mutator) =>
        replaceSelectedNode((node) => {
          if (!node.choice.prompt) {
            node.choice.prompt = { name: "", slot: "slot1", expression: null };
          }
          mutator(node.choice.prompt);
        }),
      ),
    );
  }

  const promptToggle = checkboxInput(
    Boolean(data.prompt),
    (checked) =>
      replaceSelectedNode((node) => {
        node.choice.prompt = checked ? { name: "", slot: "slot1", expression: null } : null;
      }),
    "Show prompt speaker",
  );
  root.appendChild(promptToggle);

  root.appendChild(
    field(
      "Prompt text",
      textArea(data.prompt_text ?? "", (value) =>
        replaceSelectedNode((node) => {
          node.choice.prompt_text = value === "" ? null : value;
        }),
      ),
    ),
  );

  const optionsCard = document.createElement("div");
  optionsCard.className = "subcard section-stack";
  const optHeader = document.createElement("div");
  optHeader.className = "choice-header";
  optHeader.innerHTML = `<div><p class="panel-kicker">Options</p><h2>${(data.options ?? []).length}</h2></div>`;
  const addOptionButton = document.createElement("button");
  addOptionButton.textContent = "Add option";
  addOptionButton.addEventListener("click", () =>
    replaceSelectedNode((node) => {
      node.choice.options = node.choice.options ?? [];
      node.choice.options.push(createDefaultChoiceOption());
    }),
  );
  optHeader.appendChild(addOptionButton);
  optionsCard.appendChild(optHeader);

  (data.options ?? []).forEach((option, idx) => {
    optionsCard.appendChild(buildChoiceOptionEditor(scene, option, idx));
  });
  root.appendChild(optionsCard);

  return root;
}

function buildChoiceOptionEditor(scene, option, idx) {
  const card = document.createElement("div");
  card.className = "subcard section-stack";

  const head = document.createElement("div");
  head.className = "choice-header";
  const title = document.createElement("strong");
  title.textContent = `Option ${idx + 1}`;
  const removeButton = document.createElement("button");
  removeButton.textContent = "Remove";
  removeButton.className = "danger-button";
  removeButton.addEventListener("click", () =>
    replaceSelectedNode((node) => {
      node.choice.options.splice(idx, 1);
    }),
  );
  head.append(title, removeButton);
  card.appendChild(head);

  const grid = document.createElement("div");
  grid.className = "field-grid two";
  grid.append(
    field(
      "Text",
      textInput(option.text, (value) =>
        replaceSelectedNode((node) => {
          node.choice.options[idx].text = value;
        }),
      ),
    ),
    field(
      "Next",
      buildNodeRefSelect(scene, option.next, (value) =>
        replaceSelectedNode((node) => {
          node.choice.options[idx].next = value;
        }),
      ),
    ),
  );
  card.appendChild(grid);

  card.appendChild(
    buildConditionEditor("Visible when", option.condition, (value) =>
      replaceSelectedNode((node) => {
        node.choice.options[idx].condition = value;
      }),
    ),
  );

  card.appendChild(
    buildEffectListEditor("Effects on pick", option.effects ?? [], (next) =>
      replaceSelectedNode((node) => {
        node.choice.options[idx].effects = next;
      }),
    ),
  );

  card.appendChild(
    field(
      "Legacy event id (compat)",
      numberInput(
        option.legacy_event_id ?? 0,
        (value) =>
          replaceSelectedNode((node) => {
            node.choice.options[idx].legacy_event_id = value;
          }),
        { min: 0 },
      ),
    ),
  );

  return card;
}

function buildSceneNodeBody(scene, data) {
  const root = document.createElement("div");
  root.className = "section-stack";

  const actionsCard = document.createElement("div");
  actionsCard.className = "subcard section-stack";
  const head = document.createElement("div");
  head.className = "choice-header";
  head.innerHTML = `<div><p class="panel-kicker">Actions</p><h2>${(data.actions ?? []).length}</h2></div>`;

  const addCluster = document.createElement("div");
  addCluster.className = "action-cluster";
  const addSelect = selectInput(SCENE_ACTION_KINDS, "wait", () => {});
  const addButton = document.createElement("button");
  addButton.textContent = "Add action";
  addButton.addEventListener("click", () =>
    replaceSelectedNode((node) => {
      node.scene.actions = node.scene.actions ?? [];
      node.scene.actions.push(defaultSceneActionForKind(addSelect.value));
    }),
  );
  addCluster.append(addSelect, addButton);
  head.appendChild(addCluster);
  actionsCard.appendChild(head);

  (data.actions ?? []).forEach((action, idx) => {
    actionsCard.appendChild(buildSceneActionEditor(action, idx));
  });
  root.appendChild(actionsCard);

  root.appendChild(
    field(
      "Next (after timeline)",
      buildNodeRefSelect(scene, data.next, (value) =>
        replaceSelectedNode((node) => {
          node.scene.next = value;
        }),
      ),
    ),
  );

  return root;
}

function buildSceneActionEditor(action, idx) {
  const card = document.createElement("div");
  card.className = "subcard section-stack";
  const kind = sceneActionKind(action);
  const data = action[kind];

  const head = document.createElement("div");
  head.className = "choice-header";
  const title = document.createElement("strong");
  title.textContent = `Action ${idx + 1}`;
  const controls = document.createElement("div");
  controls.className = "action-cluster";
  const kindSelect = selectInput(SCENE_ACTION_KINDS, kind, (value) =>
    replaceSelectedNode((node) => {
      node.scene.actions[idx] = defaultSceneActionForKind(value);
    }),
  );
  const removeButton = document.createElement("button");
  removeButton.textContent = "Remove";
  removeButton.className = "danger-button";
  removeButton.addEventListener("click", () =>
    replaceSelectedNode((node) => {
      node.scene.actions.splice(idx, 1);
    }),
  );
  controls.append(kindSelect, removeButton);
  head.append(title, controls);
  card.appendChild(head);

  card.appendChild(buildSceneActionFields(kind, data, idx));
  return card;
}

function buildSceneActionFields(kind, data, idx) {
  const wrapper = document.createElement("div");
  wrapper.className = "section-stack";

  const writeField = (mutator) =>
    replaceSelectedNode((node) => {
      const action = node.scene.actions[idx];
      const k = sceneActionKind(action);
      mutator(action[k]);
    });

  if (kind === "enter_character") {
    const grid = document.createElement("div");
    grid.className = "field-grid three";
    grid.append(
      field("Name", textInput(data.name ?? "", (v) => writeField((d) => { d.name = v; }))),
      field(
        "Slot",
        selectInput(SPEAKER_SLOTS, data.slot ?? "slot1", (v) => writeField((d) => { d.slot = v; })),
      ),
      field(
        "Expression",
        textInput(data.expression ?? "", (v) =>
          writeField((d) => { d.expression = v === "" ? null : v; }),
        ),
      ),
    );
    wrapper.appendChild(grid);
    wrapper.appendChild(
      field(
        "Transition (s)",
        numberInput(
          data.transition_secs ?? 0,
          (v) => writeField((d) => { d.transition_secs = v; }),
          { step: 0.05, min: 0 },
        ),
      ),
    );
  } else if (kind === "exit_character") {
    const grid = document.createElement("div");
    grid.className = "field-grid two";
    grid.append(
      field("Name", textInput(data.name ?? "", (v) => writeField((d) => { d.name = v; }))),
      field(
        "Transition (s)",
        numberInput(
          data.transition_secs ?? 0,
          (v) => writeField((d) => { d.transition_secs = v; }),
          { step: 0.05, min: 0 },
        ),
      ),
    );
    wrapper.appendChild(grid);
  } else if (kind === "set_expression") {
    const grid = document.createElement("div");
    grid.className = "field-grid two";
    grid.append(
      field("Name", textInput(data.name ?? "", (v) => writeField((d) => { d.name = v; }))),
      field(
        "Expression",
        textInput(data.expression ?? "", (v) =>
          writeField((d) => { d.expression = v === "" ? null : v; }),
        ),
      ),
    );
    wrapper.appendChild(grid);
  } else if (kind === "set_background") {
    wrapper.appendChild(
      field(
        "Background path (empty clears)",
        textInput(data ?? "", (v) =>
          replaceSelectedNode((node) => {
            node.scene.actions[idx] = { set_background: v === "" ? null : v };
          }),
        ),
      ),
    );
  } else if (kind === "play_music") {
    wrapper.appendChild(
      field(
        "Music name (empty stops)",
        textInput(data ?? "", (v) =>
          replaceSelectedNode((node) => {
            node.scene.actions[idx] = { play_music: v === "" ? null : v };
          }),
        ),
      ),
    );
  } else if (kind === "play_sfx") {
    wrapper.appendChild(
      field(
        "SFX name",
        textInput(data ?? "", (v) =>
          replaceSelectedNode((node) => {
            node.scene.actions[idx] = { play_sfx: v };
          }),
        ),
      ),
    );
  } else {
    wrapper.appendChild(
      field(
        "Seconds",
        numberInput(
          data ?? 0,
          (v) =>
            replaceSelectedNode((node) => {
              const action = node.scene.actions[idx];
              const k = sceneActionKind(action);
              action[k] = v;
            }),
          { step: 0.05, min: 0 },
        ),
      ),
    );
  }
  return wrapper;
}

function defaultSceneActionForKind(kind) {
  switch (kind) {
    case "enter_character":
      return {
        enter_character: {
          name: "",
          slot: "slot1",
          expression: null,
          transition_secs: 0,
        },
      };
    case "exit_character":
      return { exit_character: { name: "", transition_secs: 0 } };
    case "set_expression":
      return { set_expression: { name: "", expression: null } };
    case "set_background":
      return { set_background: null };
    case "play_music":
      return { play_music: null };
    case "play_sfx":
      return { play_sfx: "" };
    case "wait":
      return { wait: 1.0 };
    case "fade_out":
      return { fade_out: 0.5 };
    case "fade_in":
      return { fade_in: 0.5 };
    case "shake_screen":
      return { shake_screen: 0.4 };
    default:
      return { wait: 1.0 };
  }
}

// ---------------------------------------------------------------------------
// Speaker / condition / effect / node-ref editors
// ---------------------------------------------------------------------------

function buildSpeakerEditor(label, speaker, mutate) {
  const card = document.createElement("div");
  card.className = "subcard section-stack";
  const heading = document.createElement("p");
  heading.className = "panel-kicker";
  heading.textContent = label;
  card.appendChild(heading);

  const grid = document.createElement("div");
  grid.className = "field-grid three";
  grid.append(
    field(
      "Name",
      textInput(speaker.name ?? "", (value) =>
        mutate((s) => {
          s.name = value;
        }),
      ),
    ),
    field(
      "Slot",
      selectInput(SPEAKER_SLOTS, speaker.slot ?? "slot1", (value) =>
        mutate((s) => {
          s.slot = value;
        }),
      ),
    ),
    field(
      "Expression",
      textInput(speaker.expression ?? "", (value) =>
        mutate((s) => {
          s.expression = value === "" ? null : value;
        }),
      ),
    ),
  );
  card.appendChild(grid);
  return card;
}

function buildNodeRefSelect(scene, currentValue, onChange) {
  const select = document.createElement("select");
  const noneOption = document.createElement("option");
  noneOption.value = "";
  noneOption.textContent = "(end of dialogue)";
  noneOption.selected = currentValue == null;
  select.appendChild(noneOption);

  for (const id of Object.keys(scene.nodes ?? {})) {
    const option = document.createElement("option");
    option.value = id;
    option.textContent = id;
    option.selected = id === currentValue;
    select.appendChild(option);
  }

  // Allow free-text node id (a "future" node not yet added).
  if (currentValue && !Object.keys(scene.nodes ?? {}).includes(currentValue)) {
    const stale = document.createElement("option");
    stale.value = currentValue;
    stale.textContent = `${currentValue} (missing)`;
    stale.selected = true;
    select.appendChild(stale);
  }

  select.addEventListener("change", () => onChange(select.value === "" ? null : select.value));
  return select;
}

function buildConditionEditor(label, condition, onChange) {
  const card = document.createElement("div");
  card.className = "subcard section-stack";
  const heading = document.createElement("div");
  heading.className = "choice-header";
  const titleNode = document.createElement("p");
  titleNode.className = "panel-kicker";
  titleNode.textContent = label;
  heading.appendChild(titleNode);

  if (!condition) {
    const addButton = document.createElement("button");
    addButton.textContent = "Add condition";
    addButton.addEventListener("click", () => onChange(defaultConditionOfKind("flag")));
    heading.appendChild(addButton);
    card.appendChild(heading);
    const help = document.createElement("div");
    help.className = "help-text";
    help.textContent = "Always true while empty.";
    card.appendChild(help);
    return card;
  }

  const removeButton = document.createElement("button");
  removeButton.textContent = "Remove";
  removeButton.className = "danger-button";
  removeButton.addEventListener("click", () => onChange(null));
  heading.appendChild(removeButton);
  card.appendChild(heading);

  card.appendChild(buildConditionNode(condition, onChange));
  return card;
}

function buildConditionNode(condition, onChange) {
  const wrapper = document.createElement("div");
  wrapper.className = "section-stack";
  const kind = conditionKind(condition);
  const data = condition[kind];

  const kindRow = document.createElement("div");
  kindRow.className = "field-grid two";
  const kindSelect = selectInput(CONDITION_KINDS, kind, (value) =>
    onChange(defaultConditionOfKind(value)),
  );
  kindRow.append(field("Type", kindSelect));
  wrapper.appendChild(kindRow);

  if (kind === "flag" || kind === "not_flag") {
    wrapper.appendChild(
      field(
        "Flag name",
        textInput(data ?? "", (value) =>
          onChange({ [kind]: value }),
        ),
      ),
    );
  } else if (kind === "has_item") {
    const grid = document.createElement("div");
    grid.className = "field-grid two";
    grid.append(
      field(
        "Item id",
        numberInput(
          data.item ?? 0,
          (value) => onChange({ has_item: { ...data, item: value } }),
          { min: 0 },
        ),
      ),
      field(
        "Quantity",
        numberInput(
          data.qty ?? 1,
          (value) => onChange({ has_item: { ...data, qty: value } }),
          { min: 0 },
        ),
      ),
    );
    wrapper.appendChild(grid);
  } else if (kind === "quest_status") {
    const grid = document.createElement("div");
    grid.className = "field-grid two";
    grid.append(
      field(
        "Quest id",
        numberInput(
          data.quest ?? 0,
          (value) => onChange({ quest_status: { ...data, quest: value } }),
          { min: 0 },
        ),
      ),
      field(
        "Status",
        selectInput(QUEST_STATUS_FILTERS, data.status ?? "active", (value) =>
          onChange({ quest_status: { ...data, status: value } }),
        ),
      ),
    );
    wrapper.appendChild(grid);
  } else if (kind === "reputation_at_least") {
    wrapper.appendChild(
      buildReputationTargetEditor(data.target, (target) =>
        onChange({ reputation_at_least: { ...data, target } }),
      ),
    );
    wrapper.appendChild(
      field(
        "Minimum",
        numberInput(data.min ?? 0, (value) =>
          onChange({ reputation_at_least: { ...data, min: value } }),
        ),
      ),
    );
  } else if (kind === "all" || kind === "any") {
    const list = data ?? [];
    const card = document.createElement("div");
    card.className = "subcard section-stack";
    const head = document.createElement("div");
    head.className = "choice-header";
    head.innerHTML = `<strong>${kind === "all" ? "All of" : "Any of"} (${list.length})</strong>`;
    const addButton = document.createElement("button");
    addButton.textContent = "Add child";
    addButton.addEventListener("click", () =>
      onChange({ [kind]: [...list, defaultConditionOfKind("flag")] }),
    );
    head.appendChild(addButton);
    card.appendChild(head);
    list.forEach((child, idx) => {
      const childCard = document.createElement("div");
      childCard.className = "subcard section-stack";
      const childHead = document.createElement("div");
      childHead.className = "choice-header";
      childHead.innerHTML = `<strong>#${idx + 1}</strong>`;
      const childRemove = document.createElement("button");
      childRemove.textContent = "Remove";
      childRemove.className = "danger-button";
      childRemove.addEventListener("click", () => {
        const next = [...list];
        next.splice(idx, 1);
        onChange({ [kind]: next });
      });
      childHead.appendChild(childRemove);
      childCard.appendChild(childHead);
      childCard.appendChild(
        buildConditionNode(child, (replacement) => {
          const next = [...list];
          next[idx] = replacement;
          onChange({ [kind]: next });
        }),
      );
      card.appendChild(childCard);
    });
    wrapper.appendChild(card);
  } else if (kind === "not") {
    wrapper.appendChild(
      buildConditionNode(data ?? defaultConditionOfKind("flag"), (replacement) =>
        onChange({ not: replacement }),
      ),
    );
  }

  return wrapper;
}

function defaultConditionOfKind(kind) {
  switch (kind) {
    case "flag":
      return { flag: "" };
    case "not_flag":
      return { not_flag: "" };
    case "has_item":
      return { has_item: { item: 0, qty: 1 } };
    case "quest_status":
      return { quest_status: { quest: 0, status: "active" } };
    case "reputation_at_least":
      return { reputation_at_least: { target: { local_governor: null }, min: 0 } };
    case "all":
      return { all: [] };
    case "any":
      return { any: [] };
    case "not":
      return { not: { flag: "" } };
    default:
      return { flag: "" };
  }
}

function buildReputationTargetEditor(target, onChange) {
  const wrapper = document.createElement("div");
  wrapper.className = "section-stack";
  const kind = reputationTargetKind(target);
  const data = target[kind];

  const grid = document.createElement("div");
  grid.className = "field-grid two";
  grid.append(
    field(
      "Target",
      selectInput(REPUTATION_TARGET_KINDS, kind, (value) =>
        onChange(defaultReputationTargetOfKind(value)),
      ),
    ),
  );
  wrapper.appendChild(grid);

  if (kind === "governor") {
    wrapper.appendChild(
      field(
        "City id",
        numberInput(
          data.city_id ?? 0,
          (value) => onChange({ governor: { city_id: value } }),
          { min: 0 },
        ),
      ),
    );
  } else if (kind === "merchant") {
    wrapper.appendChild(
      field(
        "Merchant id",
        numberInput(
          data.merchant_id ?? 0,
          (value) => onChange({ merchant: { merchant_id: value } }),
          { min: 0 },
        ),
      ),
    );
  } else if (kind === "clan") {
    wrapper.appendChild(
      field(
        "Clan name",
        textInput(data.name ?? "", (value) => onChange({ clan: { name: value } })),
      ),
    );
  }
  return wrapper;
}

function defaultReputationTargetOfKind(kind) {
  switch (kind) {
    case "local_governor":
      return { local_governor: null };
    case "local_merchant":
      return { local_merchant: null };
    case "local_clan":
      return { local_clan: null };
    case "governor":
      return { governor: { city_id: 0 } };
    case "merchant":
      return { merchant: { merchant_id: 0 } };
    case "clan":
      return { clan: { name: "" } };
    default:
      return { local_governor: null };
  }
}

function buildEffectListEditor(label, effects, onChange) {
  const card = document.createElement("div");
  card.className = "subcard section-stack";
  const head = document.createElement("div");
  head.className = "choice-header";
  head.innerHTML = `<p class="panel-kicker">${label}</p>`;

  const addCluster = document.createElement("div");
  addCluster.className = "action-cluster";
  const addSelect = selectInput(EFFECT_KINDS, "set_flag", () => {});
  const addButton = document.createElement("button");
  addButton.textContent = "Add effect";
  addButton.addEventListener("click", () =>
    onChange([...(effects ?? []), defaultDialogueEffectOfKind(addSelect.value)]),
  );
  addCluster.append(addSelect, addButton);
  head.appendChild(addCluster);
  card.appendChild(head);

  if (!effects || effects.length === 0) {
    const help = document.createElement("div");
    help.className = "help-text";
    help.textContent = "No effects.";
    card.appendChild(help);
    return card;
  }

  effects.forEach((effect, idx) => {
    const sub = document.createElement("div");
    sub.className = "subcard section-stack";
    const subHead = document.createElement("div");
    subHead.className = "choice-header";
    const title = document.createElement("strong");
    title.textContent = `Effect ${idx + 1}`;
    const controls = document.createElement("div");
    controls.className = "action-cluster";
    const kindSelect = selectInput(EFFECT_KINDS, dialogueEffectKind(effect), (value) => {
      const next = [...effects];
      next[idx] = defaultDialogueEffectOfKind(value);
      onChange(next);
    });
    const removeButton = document.createElement("button");
    removeButton.textContent = "Remove";
    removeButton.className = "danger-button";
    removeButton.addEventListener("click", () => {
      const next = [...effects];
      next.splice(idx, 1);
      onChange(next);
    });
    controls.append(kindSelect, removeButton);
    subHead.append(title, controls);
    sub.appendChild(subHead);
    sub.appendChild(buildDialogueEffectFields(effect, (replacement) => {
      const next = [...effects];
      next[idx] = replacement;
      onChange(next);
    }));
    card.appendChild(sub);
  });
  return card;
}

function buildDialogueEffectFields(effect, onChange) {
  const wrapper = document.createElement("div");
  wrapper.className = "section-stack";
  const kind = dialogueEffectKind(effect);
  const data = effect[kind];

  if (kind === "set_flag" || kind === "clear_flag") {
    wrapper.appendChild(
      field(
        "Flag name",
        textInput(data ?? "", (value) => onChange({ [kind]: value })),
      ),
    );
  } else if (kind === "reputation") {
    wrapper.appendChild(
      buildReputationTargetEditor(data.target, (target) =>
        onChange({ reputation: { ...data, target } }),
      ),
    );
    const grid = document.createElement("div");
    grid.className = "field-grid two";
    grid.append(
      field(
        "Delta",
        numberInput(data.delta ?? 0, (value) =>
          onChange({ reputation: { ...data, delta: value } }),
        ),
      ),
      field(
        "Reason",
        textInput(data.reason ?? "", (value) =>
          onChange({ reputation: { ...data, reason: value } }),
        ),
      ),
    );
    wrapper.appendChild(grid);
  } else if (kind === "give_item" || kind === "take_item") {
    const grid = document.createElement("div");
    grid.className = "field-grid two";
    grid.append(
      field(
        "Item id",
        numberInput(
          data.item ?? 0,
          (value) => onChange({ [kind]: { ...data, item: value } }),
          { min: 0 },
        ),
      ),
      field(
        "Quantity",
        numberInput(
          data.qty ?? 1,
          (value) => onChange({ [kind]: { ...data, qty: value } }),
          { min: 0 },
        ),
      ),
    );
    wrapper.appendChild(grid);
  } else if (kind === "give_coin" || kind === "take_coin") {
    wrapper.appendChild(
      field(
        "Amount",
        numberInput(
          data ?? 0,
          (value) => onChange({ [kind]: value }),
          { min: 0 },
        ),
      ),
    );
  } else if (kind === "start_quest") {
    wrapper.appendChild(
      field(
        "Quest id",
        numberInput(
          data ?? 0,
          (value) => onChange({ start_quest: value }),
          { min: 0 },
        ),
      ),
    );
  } else if (kind === "advance_objective") {
    const grid = document.createElement("div");
    grid.className = "field-grid two";
    grid.append(
      field(
        "Quest id",
        numberInput(
          data.quest ?? 0,
          (value) => onChange({ advance_objective: { ...data, quest: value } }),
          { min: 0 },
        ),
      ),
      field(
        "Objective id",
        numberInput(
          data.objective ?? 0,
          (value) => onChange({ advance_objective: { ...data, objective: value } }),
          { min: 0 },
        ),
      ),
    );
    wrapper.appendChild(grid);
  } else if (kind === "accept_contract") {
    const help = document.createElement("div");
    help.className = "help-text";
    help.textContent = "Inserts the Bound marker on the player.";
    wrapper.appendChild(help);
  } else if (kind === "play_sfx") {
    wrapper.appendChild(
      field(
        "SFX name",
        textInput(data ?? "", (value) => onChange({ play_sfx: value })),
      ),
    );
  } else if (kind === "play_music") {
    wrapper.appendChild(
      field(
        "Music name (empty stops)",
        textInput(data ?? "", (value) =>
          onChange({ play_music: value === "" ? null : value }),
        ),
      ),
    );
  } else if (kind === "spawn_interactable") {
    const grid = document.createElement("div");
    grid.className = "field-grid three";
    grid.append(
      field(
        "Kind",
        textInput(data.kind ?? "", (value) =>
          onChange({ spawn_interactable: { ...data, kind: value } }),
        ),
      ),
      field(
        "X",
        numberInput(data.x ?? 0, (value) =>
          onChange({ spawn_interactable: { ...data, x: value } }),
        ),
      ),
      field(
        "Y",
        numberInput(data.y ?? 0, (value) =>
          onChange({ spawn_interactable: { ...data, y: value } }),
        ),
      ),
    );
    wrapper.appendChild(grid);
  } else if (kind === "despawn_interactable") {
    wrapper.appendChild(
      field(
        "Interactable name",
        textInput(data.name ?? "", (value) =>
          onChange({ despawn_interactable: { name: value } }),
        ),
      ),
    );
  } else if (kind === "change_scene") {
    wrapper.appendChild(
      field(
        "Target scene id",
        textInput(data ?? "", (value) => onChange({ change_scene: value })),
      ),
    );
  }
  return wrapper;
}

function defaultDialogueEffectOfKind(kind) {
  switch (kind) {
    case "set_flag":
      return { set_flag: "" };
    case "clear_flag":
      return { clear_flag: "" };
    case "reputation":
      return {
        reputation: {
          target: { local_governor: null },
          delta: 0,
          reason: "",
        },
      };
    case "give_item":
      return { give_item: { item: 0, qty: 1 } };
    case "take_item":
      return { take_item: { item: 0, qty: 1 } };
    case "give_coin":
      return { give_coin: 0 };
    case "take_coin":
      return { take_coin: 0 };
    case "start_quest":
      return { start_quest: 0 };
    case "advance_objective":
      return { advance_objective: { quest: 0, objective: 0 } };
    case "accept_contract":
      return { accept_contract: null };
    case "play_sfx":
      return { play_sfx: "" };
    case "play_music":
      return { play_music: null };
    case "spawn_interactable":
      return { spawn_interactable: { kind: "", x: 0, y: 0 } };
    case "despawn_interactable":
      return { despawn_interactable: { name: "" } };
    case "change_scene":
      return { change_scene: "" };
    default:
      return { set_flag: "" };
  }
}

// ---------------------------------------------------------------------------
// Ability editor (unchanged)
// ---------------------------------------------------------------------------

function buildAbilityEditor() {
  const container = document.createElement("section");
  container.className = "card section-stack";
  const selected =
    state.selectedAbilityIndex === null ? null : state.abilities[state.selectedAbilityIndex];

  if (!selected) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = "Select an ability to edit or create a new one.";
    container.appendChild(empty);
    return container;
  }

  const header = document.createElement("div");
  header.className = "panel-header";
  const heading = document.createElement("div");
  heading.innerHTML = `<p class="panel-kicker">Ability</p><h2>${
    selected.name || `Ability ${selected.id}`
  }</h2>`;
  const actions = document.createElement("div");
  actions.className = "action-cluster";

  const duplicateButton = document.createElement("button");
  duplicateButton.textContent = "Duplicate";
  duplicateButton.addEventListener("click", () => {
    const duplicate = clone(selected);
    duplicate.id = nextFreeAbilityId(state.abilities, (selected.id >> 8) & 0xff);
    duplicate.name = `${selected.name} Copy`.trim();
    state.abilities.push(duplicate);
    state.selectedAbilityIndex = state.abilities.length - 1;
    setDirty(true);
    render();
  });

  const deleteButton = document.createElement("button");
  deleteButton.textContent = "Delete";
  deleteButton.className = "danger-button";
  deleteButton.addEventListener("click", () => {
    state.abilities.splice(state.selectedAbilityIndex, 1);
    state.selectedAbilityIndex =
      state.abilities.length === 0
        ? null
        : Math.min(state.selectedAbilityIndex, state.abilities.length - 1);
    setDirty(true);
    render();
  });

  actions.append(duplicateButton, deleteButton);
  header.append(heading, actions);
  container.appendChild(header);

  const basics = document.createElement("div");
  basics.className = "editor-card section-stack";
  basics.innerHTML = `<p class="panel-kicker">Core</p>`;
  const level = (selected.id >> 8) & 0xff;
  const shapeKind = abilityShapeKind(selected.shape);
  const basicGrid = document.createElement("div");
  basicGrid.className = "field-grid four";
  basicGrid.append(
    field("ID", numberInput(selected.id, (value) => replaceSelectedAbility((a) => { a.id = value; }))),
    field(
      "Packed Level",
      numberInput(
        level,
        (value) =>
          replaceSelectedAbility((a) => {
            const subId = a.id & 0xff;
            a.id = ((value & 0xff) << 8) | subId;
          }),
        { min: 0, max: 255 },
      ),
    ),
    field(
      "Auto ID",
      (() => {
        const button = document.createElement("button");
        button.textContent = "Generate";
        button.addEventListener("click", () =>
          replaceSelectedAbility((a) => {
            a.id = nextFreeAbilityId(state.abilities, (a.id >> 8) & 0xff);
          }),
        );
        return button;
      })(),
    ),
    field(
      "Next ID",
      (() => {
        const values = ["", ...state.abilities.map((a) => String(a.id))];
        const select = selectInput(
          values,
          selected.next_id === null ? "" : String(selected.next_id),
          (value) => replaceSelectedAbility((a) => { a.next_id = value === "" ? null : Number(value); }),
        );
        select.options[0].textContent = "None";
        return select;
      })(),
    ),
  );

  const textGrid = document.createElement("div");
  textGrid.className = "field-grid two";
  textGrid.append(
    field("Name", textInput(selected.name, (value) => replaceSelectedAbility((a) => { a.name = value; }))),
    field(
      "Shape",
      selectInput(["Select", "Radius", "Line", "Cone"], shapeKind, (value) =>
        replaceSelectedAbility((a) => { a.shape = shapeForKind(value); }),
      ),
    ),
  );

  const numbers = document.createElement("div");
  numbers.className = "field-grid four";
  numbers.append(
    field("Health Cost", numberInput(selected.health_cost, (v) => replaceSelectedAbility((a) => { a.health_cost = v; }))),
    field("Magic Cost", numberInput(selected.magic_cost, (v) => replaceSelectedAbility((a) => { a.magic_cost = v; }))),
    field("AP Cost", numberInput(selected.action_point_cost, (v) => replaceSelectedAbility((a) => { a.action_point_cost = v; }))),
    field("Cooldown", numberInput(selected.cooldown, (v) => replaceSelectedAbility((a) => { a.cooldown = v; }), { min: 0 })),
  );

  const durationGrid = document.createElement("div");
  durationGrid.className = "field-grid two";
  durationGrid.append(
    field("Duration", numberInput(selected.duration, (v) => replaceSelectedAbility((a) => { a.duration = v; }), { min: 0 })),
    field("Targets", numberInput(selected.targets, (v) => replaceSelectedAbility((a) => { a.targets = v; }), { min: 0 })),
  );

  basics.append(
    basicGrid,
    textGrid,
    numbers,
    durationGrid,
    field(
      "Description",
      textArea(selected.description, (value) =>
        replaceSelectedAbility((a) => { a.description = value; }),
      ),
    ),
  );

  const shapeCard = document.createElement("div");
  shapeCard.className = "editor-card section-stack";
  shapeCard.innerHTML = `<p class="panel-kicker">Shape Settings</p>`;
  if (shapeKind === "Radius") {
    shapeCard.append(
      field(
        "Radius",
        numberInput(selected.shape.Radius, (value) =>
          replaceSelectedAbility((a) => { a.shape = { Radius: value }; }),
          { step: 0.1 },
        ),
      ),
    );
  } else if (shapeKind === "Line") {
    const grid = document.createElement("div");
    grid.className = "field-grid two";
    grid.append(
      field("Length", numberInput(selected.shape.Line.length, (value) => replaceSelectedAbility((a) => { a.shape.Line.length = value; }), { step: 0.1 })),
      field("Thickness", numberInput(selected.shape.Line.thickness, (value) => replaceSelectedAbility((a) => { a.shape.Line.thickness = value; }), { step: 0.1 })),
    );
    shapeCard.append(grid);
  } else if (shapeKind === "Cone") {
    const grid = document.createElement("div");
    grid.className = "field-grid two";
    grid.append(
      field("Angle", numberInput(selected.shape.Cone.angle, (value) => replaceSelectedAbility((a) => { a.shape.Cone.angle = value; }), { step: 0.1 })),
      field("Radius", numberInput(selected.shape.Cone.radius, (value) => replaceSelectedAbility((a) => { a.shape.Cone.radius = value; }), { step: 0.1 })),
    );
    shapeCard.append(grid);
  } else {
    const note = document.createElement("div");
    note.className = "help-text";
    note.textContent = "Select shape uses no extra fields.";
    shapeCard.append(note);
  }

  const effectsCard = document.createElement("div");
  effectsCard.className = "editor-card section-stack";
  const effectsHeader = document.createElement("div");
  effectsHeader.className = "effect-header";
  effectsHeader.innerHTML = `<div><p class="panel-kicker">Effects</p><h2>Effect Stack</h2></div>`;
  const addEffectSelect = selectInput(["Heal", "Damage", "Buff"], "Heal", () => {});
  const addEffectButton = document.createElement("button");
  addEffectButton.textContent = "Add Effect";
  addEffectButton.addEventListener("click", () => {
    replaceSelectedAbility((ability) => {
      ability.effects.push(effectForKind(addEffectSelect.value));
    });
  });
  const addCluster = document.createElement("div");
  addCluster.className = "action-cluster";
  addCluster.append(addEffectSelect, addEffectButton);
  effectsHeader.appendChild(addCluster);
  effectsCard.appendChild(effectsHeader);

  if ((selected.effects ?? []).length === 0) {
    const empty = document.createElement("div");
    empty.className = "help-text";
    empty.textContent = "No effects yet.";
    effectsCard.appendChild(empty);
  }

  (selected.effects ?? []).forEach((effect, index) => {
    const kind = effectKind(effect);
    const card = document.createElement("div");
    card.className = "subcard section-stack";
    const headerRow = document.createElement("div");
    headerRow.className = "effect-header";
    const title = document.createElement("strong");
    title.textContent = `Effect ${index + 1}`;
    const controls = document.createElement("div");
    controls.className = "action-cluster";
    const kindSelect = selectInput(["Heal", "Damage", "Buff"], kind, (value) =>
      replaceSelectedAbility((ability) => {
        ability.effects[index] = effectForKind(value);
      }),
    );
    const removeButton = document.createElement("button");
    removeButton.textContent = "Remove";
    removeButton.className = "danger-button";
    removeButton.addEventListener("click", () =>
      replaceSelectedAbility((ability) => { ability.effects.splice(index, 1); }),
    );
    controls.append(kindSelect, removeButton);
    headerRow.append(title, controls);
    card.appendChild(headerRow);

    if (kind === "Heal") {
      const data = effect.Heal;
      const grid = document.createElement("div");
      grid.className = "field-grid three";
      grid.append(
        field("Floor", numberInput(data.floor, (v) => replaceSelectedAbility((a) => { a.effects[index].Heal.floor = v; }))),
        field("Ceiling", numberInput(data.ceiling, (v) => replaceSelectedAbility((a) => { a.effects[index].Heal.ceiling = v; }))),
        field(
          "Scaled With",
          selectInput(STAT_OPTIONS, data.scaled_with, (v) =>
            replaceSelectedAbility((a) => { a.effects[index].Heal.scaled_with = v; }),
          ),
        ),
      );
      card.appendChild(grid);
    }
    if (kind === "Damage") {
      const data = effect.Damage;
      const top = document.createElement("div");
      top.className = "field-grid four";
      top.append(
        field("Floor", numberInput(data.floor, (v) => replaceSelectedAbility((a) => { a.effects[index].Damage.floor = v; }))),
        field("Ceiling", numberInput(data.ceiling, (v) => replaceSelectedAbility((a) => { a.effects[index].Damage.ceiling = v; }))),
        field(
          "Damage Type",
          selectInput(DAMAGE_TYPE_OPTIONS, data.damage_type, (v) =>
            replaceSelectedAbility((a) => { a.effects[index].Damage.damage_type = v; }),
          ),
        ),
        field(
          "Scaled With",
          selectInput(STAT_OPTIONS, data.scaled_with, (v) =>
            replaceSelectedAbility((a) => { a.effects[index].Damage.scaled_with = v; }),
          ),
        ),
      );
      const bottom = document.createElement("div");
      bottom.className = "field-grid two";
      bottom.append(
        field(
          "Defended With",
          selectInput(STAT_OPTIONS, data.defended_with, (v) =>
            replaceSelectedAbility((a) => { a.effects[index].Damage.defended_with = v; }),
          ),
        ),
      );
      card.append(top, bottom);
    }
    if (kind === "Buff") {
      const data = effect.Buff;
      const grid = document.createElement("div");
      grid.className = "field-grid four";
      grid.append(
        field("Stat", selectInput(STAT_OPTIONS, data.stat, (v) => replaceSelectedAbility((a) => { a.effects[index].Buff.stat = v; }))),
        field(
          "Multiplier",
          numberInput(data.multiplier, (v) => replaceSelectedAbility((a) => { a.effects[index].Buff.multiplier = v; }), { step: 0.05 }),
        ),
        field(
          "Scaled With",
          selectInput(STAT_OPTIONS, data.scaled_with, (v) =>
            replaceSelectedAbility((a) => { a.effects[index].Buff.scaled_with = v; }),
          ),
        ),
        checkboxInput(
          Boolean(data.effects),
          (checked) =>
            replaceSelectedAbility((a) => { a.effects[index].Buff.effects = checked ? [] : null; }),
          "Trigger linked abilities",
        ),
      );
      card.appendChild(grid);

      if (data.effects) {
        const linked = field(
          "Linked Ability IDs",
          textInput(data.effects.join(", "), (value) =>
            replaceSelectedAbility((ability) => {
              ability.effects[index].Buff.effects = value
                .split(",")
                .map((part) => part.trim())
                .filter(Boolean)
                .map((part) => Number(part));
            }),
          ),
        );
        const note = document.createElement("div");
        note.className = "help-text";
        note.textContent = "Comma-separated list of ability ids.";
        linked.appendChild(note);
        card.appendChild(linked);
      }
    }

    effectsCard.appendChild(card);
  });

  container.append(basics, shapeCard, effectsCard);
  return container;
}

// ---------------------------------------------------------------------------
// Top-level render
// ---------------------------------------------------------------------------

function renderEditor() {
  elements.editorRoot.innerHTML = "";
  elements.editorRoot.appendChild(
    state.editor === "abilities" ? buildAbilityEditor() : buildSceneEditor(),
  );
}

function renderValidationMessages() {
  const validation = activeValidationMessages();
  if (validation.length === 0) return;
  for (const message of validation) {
    state.messages.unshift({
      id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
      kind: message === "No issues detected." ? "success" : "warning",
      text: message,
    });
  }
  state.messages = state.messages.slice(0, 20);
  renderMessages();
}

function render() {
  renderSummary();
  renderList();
  renderEditor();
}

// ---------------------------------------------------------------------------
// I/O
// ---------------------------------------------------------------------------

async function loadFromServer() {
  const response = await fetch(apiUrl(), { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`GET ${apiUrl()} → ${response.status}`);
  }
  const data = await response.json();
  if (!Array.isArray(data)) {
    throw new Error("server returned non-array payload");
  }
  if (state.editor === "abilities") {
    state.abilities = data;
    state.selectedAbilityIndex = data.length > 0 ? 0 : null;
  } else {
    state.scenes = data;
    state.selectedSceneIndex = data.length > 0 ? 0 : null;
    state.selectedNodeId = data.length > 0 ? data[0].start ?? null : null;
  }
  state.loaded[state.editor] = true;
  state.validationMessages[state.editor] = [];
  setDirty(false);
  pushMessage("success", `Loaded ${state.editor} from server`);
  render();
}

async function saveToServer() {
  const response = await fetch(apiUrl(), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(currentEntries()),
  });
  if (!response.ok) {
    const errorBody = await response.text();
    throw new Error(`POST ${apiUrl()} → ${response.status}: ${errorBody}`);
  }
  setDirty(false);
  pushMessage("success", `Saved ${state.editor} as RON on server`);
  render();
}

function downloadCurrent() {
  const blob = new Blob([JSON.stringify(currentEntries(), null, 2)], {
    type: "application/json",
  });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = defaultDownloadName().replace(/\.ron$/, ".json");
  anchor.click();
  URL.revokeObjectURL(url);
  pushMessage("success", "Downloaded local JSON snapshot");
}

function runValidation() {
  state.validationMessages[state.editor] =
    state.editor === "abilities" ? validateAbilities(state.abilities) : validateScenes(state.scenes);
  render();
  renderValidationMessages();
}

function addNewEntry() {
  if (state.editor === "abilities") {
    state.abilities.push(createDefaultAbility());
    state.selectedAbilityIndex = state.abilities.length - 1;
  } else {
    const scene = createDefaultScene();
    state.scenes.push(scene);
    state.selectedSceneIndex = state.scenes.length - 1;
    state.selectedNodeId = scene.start;
  }
  setDirty(true);
  render();
}

async function switchEditor(editor) {
  state.editor = editor;
  state.search = "";
  elements.searchInput.value = "";
  if (!state.loaded[editor]) {
    try {
      await loadFromServer();
      return;
    } catch (error) {
      pushMessage("error", error.message);
    }
  }
  render();
}

elements.modeButtons.forEach((button) => {
  button.addEventListener("click", () => switchEditor(button.dataset.editor));
});

elements.searchInput.addEventListener("input", () => {
  state.search = elements.searchInput.value;
  renderList();
});

elements.newEntryButton.addEventListener("click", addNewEntry);
elements.reloadButton.addEventListener("click", () => {
  loadFromServer().catch((error) => pushMessage("error", error.message));
});
elements.saveButton.addEventListener("click", () => {
  saveToServer().catch((error) => pushMessage("error", error.message));
});
elements.downloadButton.addEventListener("click", downloadCurrent);
elements.validateButton.addEventListener("click", runValidation);

pushMessage("success", "Connected to Rust editor server.");
loadFromServer().catch((error) => pushMessage("error", error.message));
