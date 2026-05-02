const STAT_OPTIONS = ["Mind", "Agility", "Strength", "Morale", "Lethality"];
const DAMAGE_TYPE_OPTIONS = ["Physical", "Fire", "Ice", "Lightning"];

// All RON parsing/serialization is handled by the Rust editor server
// (see src/bin/editor_server.rs). The JS UI only speaks JSON to the API.
const API = {
  abilities: "/api/abilities",
  dialogues: "/api/dialogues",
};

const state = {
  editor: "abilities",
  abilities: [],
  dialogues: [],
  selectedAbilityIndex: null,
  selectedDialogueIndex: null,
  search: "",
  dirty: { abilities: false, dialogues: false },
  loaded: { abilities: false, dialogues: false },
  messages: [],
  validationMessages: { abilities: [], dialogues: [] },
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

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

function currentEntries() {
  return state.editor === "abilities" ? state.abilities : state.dialogues;
}

function currentSelectionIndex() {
  return state.editor === "abilities"
    ? state.selectedAbilityIndex
    : state.selectedDialogueIndex;
}

function setSelectionIndex(index) {
  if (state.editor === "abilities") {
    state.selectedAbilityIndex = index;
  } else {
    state.selectedDialogueIndex = index;
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
  return state.editor === "abilities" ? API.abilities : API.dialogues;
}

function defaultDownloadName() {
  return state.editor === "abilities" ? "abilities.ron" : "dialogues.ron";
}

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

function createDefaultDialogue() {
  return {
    id: uniqueDialogueId(state.dialogues),
    speaker: "",
    text: "",
    next: null,
    choices: null,
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

function uniqueDialogueId(dialogues) {
  const ids = new Set(dialogues.map((dialogue) => dialogue.id));
  let index = dialogues.length + 1;
  while (ids.has(`Dialogue_${index}`)) {
    index += 1;
  }
  return `Dialogue_${index}`;
}

// Server returns Ability.shape as either "Select" (unit variant) or
// `{Radius: 3.5}` / `{Cone: {...}}` (non-unit variant). Same for effects.
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

function validateAbilities(abilities) {
  const messages = [];
  const ids = new Set();

  for (const ability of abilities) {
    if (ids.has(ability.id)) {
      messages.push(`Duplicate ability id: ${ability.id}`);
    }
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

function validateDialogues(dialogues) {
  const messages = [];
  const ids = new Set(dialogues.map((dialogue) => dialogue.id));
  const adjacency = new Map(dialogues.map((dialogue) => [dialogue.id, []]));
  const incoming = new Map(dialogues.map((dialogue) => [dialogue.id, 0]));

  for (const dialogue of dialogues) {
    if (dialogue.next && !ids.has(dialogue.next)) {
      messages.push(`Dialogue "${dialogue.id}" has invalid next reference "${dialogue.next}"`);
    }
    if (dialogue.next && ids.has(dialogue.next)) {
      adjacency.get(dialogue.id).push(dialogue.next);
      incoming.set(dialogue.next, (incoming.get(dialogue.next) ?? 0) + 1);
    }

    for (const choice of dialogue.choices ?? []) {
      if (choice.next && !ids.has(choice.next)) {
        messages.push(
          `Dialogue "${dialogue.id}" has choice "${choice.text}" pointing to unknown id "${choice.next}"`,
        );
        continue;
      }
      if (choice.next) {
        adjacency.get(dialogue.id).push(choice.next);
        incoming.set(choice.next, (incoming.get(choice.next) ?? 0) + 1);
      }
    }
  }

  if (dialogues.length > 0) {
    const visited = new Set();
    const stack = [dialogues[0].id];
    while (stack.length > 0) {
      const current = stack.pop();
      if (visited.has(current)) continue;
      visited.add(current);
      for (const nextId of adjacency.get(current) ?? []) {
        stack.push(nextId);
      }
    }
    const unreachable = dialogues
      .map((dialogue) => dialogue.id)
      .filter((id) => !visited.has(id));
    if (unreachable.length > 0) {
      messages.push(`Unreachable dialogues: ${unreachable.join(", ")}`);
    }
    const cyclePath = detectDialogueCycle(adjacency);
    if (cyclePath) {
      messages.push(`Cycle detected: ${cyclePath.join(" -> ")}`);
    }
  }

  return messages.length > 0 ? messages : ["No issues detected."];
}

function detectDialogueCycle(adjacency) {
  const visiting = new Set();
  const visited = new Set();

  function walk(node, path) {
    if (visiting.has(node)) {
      const cycleStart = path.indexOf(node);
      return [...path.slice(cycleStart), node];
    }
    if (visited.has(node)) return null;

    visiting.add(node);
    path.push(node);
    for (const nextNode of adjacency.get(node) ?? []) {
      const result = walk(nextNode, path);
      if (result) return result;
    }
    path.pop();
    visiting.delete(node);
    visited.add(node);
    return null;
  }

  for (const node of adjacency.keys()) {
    const result = walk(node, []);
    if (result) return result;
  }
  return null;
}

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
          ? `${entry.id} ${entry.name} ${entry.description}`
          : `${entry.id} ${entry.speaker} ${entry.text}`;
      return haystack.toLowerCase().includes(search);
    });

  if (filteredEntries.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = "No entries match the current filter.";
    elements.entryList.appendChild(empty);
    return;
  }

  for (const { entry, index } of filteredEntries) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = `entry-row${selectedIndex === index ? " selected" : ""}`;
    row.addEventListener("click", () => {
      setSelectionIndex(index);
      render();
    });

    const header = document.createElement("div");
    header.className = "entry-row-header";

    const title = document.createElement("div");
    title.className = "entry-row-title";
    title.textContent =
      state.editor === "abilities" ? entry.name || `Ability ${entry.id}` : entry.id;

    const token = document.createElement("span");
    token.className = "token";
    token.textContent =
      state.editor === "abilities" ? `#${entry.id}` : entry.speaker || "Narration";

    const subtitle = document.createElement("div");
    subtitle.className = "muted";
    subtitle.textContent =
      state.editor === "abilities"
        ? entry.description || "No description"
        : entry.text || "No dialogue text";

    header.append(title, token);
    row.append(header, subtitle);
    elements.entryList.appendChild(row);
  }
}

function renderSummary() {
  const entries = currentEntries();
  const selectedIndex = currentSelectionIndex();
  const selected = selectedIndex === null ? null : entries[selectedIndex];
  elements.listTitle.textContent =
    state.editor === "abilities" ? "Abilities" : "Dialogues";
  elements.newEntryButton.textContent =
    state.editor === "abilities" ? "New Ability" : "New Dialogue";
  elements.fileLabel.textContent = state.loaded[state.editor]
    ? `${state.editor === "abilities" ? "abilities" : "dialogues"} loaded from server`
    : "Backed by Rust editor server";
  elements.dirtyLabel.textContent = state.dirty[state.editor]
    ? "Unsaved changes"
    : "No unsaved changes";
  elements.totalCount.textContent = String(entries.length);
  elements.selectedLabel.textContent = selected
    ? state.editor === "abilities"
      ? selected.name || `Ability ${selected.id}`
      : selected.id
    : "None";
  const validationMessages = activeValidationMessages();
  const issueCount =
    validationMessages.length === 1 && validationMessages[0] === "No issues detected."
      ? 0
      : validationMessages.length;
  elements.validationCount.textContent = `${issueCount} issue${issueCount === 1 ? "" : "s"}`;

  for (const button of elements.modeButtons) {
    button.classList.toggle("active", button.dataset.editor === state.editor);
  }
}

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

function replaceSelectedDialogue(mutator) {
  if (state.selectedDialogueIndex === null) return;
  const current = state.dialogues[state.selectedDialogueIndex];
  if (!current) return;
  const next = clone(current);
  mutator(next);
  state.dialogues[state.selectedDialogueIndex] = next;
  setDirty(true);
  render();
}

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
    field(
      "ID",
      numberInput(selected.id, (value) =>
        replaceSelectedAbility((ability) => {
          ability.id = value;
        }),
      ),
    ),
    field(
      "Packed Level",
      numberInput(
        level,
        (value) =>
          replaceSelectedAbility((ability) => {
            const subId = ability.id & 0xff;
            ability.id = ((value & 0xff) << 8) | subId;
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
          replaceSelectedAbility((ability) => {
            ability.id = nextFreeAbilityId(state.abilities, (ability.id >> 8) & 0xff);
          }),
        );
        return button;
      })(),
    ),
    field(
      "Next ID",
      (() => {
        const values = ["", ...state.abilities.map((ability) => String(ability.id))];
        const select = selectInput(
          values,
          selected.next_id === null ? "" : String(selected.next_id),
          (value) =>
            replaceSelectedAbility((ability) => {
              ability.next_id = value === "" ? null : Number(value);
            }),
        );
        select.options[0].textContent = "None";
        return select;
      })(),
    ),
  );

  const textGrid = document.createElement("div");
  textGrid.className = "field-grid two";
  textGrid.append(
    field(
      "Name",
      textInput(selected.name, (value) =>
        replaceSelectedAbility((ability) => {
          ability.name = value;
        }),
      ),
    ),
    field(
      "Shape",
      selectInput(["Select", "Radius", "Line", "Cone"], shapeKind, (value) =>
        replaceSelectedAbility((ability) => {
          ability.shape = shapeForKind(value);
        }),
      ),
    ),
  );

  const numbers = document.createElement("div");
  numbers.className = "field-grid four";
  numbers.append(
    field(
      "Health Cost",
      numberInput(selected.health_cost, (value) =>
        replaceSelectedAbility((ability) => {
          ability.health_cost = value;
        }),
      ),
    ),
    field(
      "Magic Cost",
      numberInput(selected.magic_cost, (value) =>
        replaceSelectedAbility((ability) => {
          ability.magic_cost = value;
        }),
      ),
    ),
    field(
      "AP Cost",
      numberInput(selected.action_point_cost, (value) =>
        replaceSelectedAbility((ability) => {
          ability.action_point_cost = value;
        }),
      ),
    ),
    field(
      "Cooldown",
      numberInput(
        selected.cooldown,
        (value) =>
          replaceSelectedAbility((ability) => {
            ability.cooldown = value;
          }),
        { min: 0 },
      ),
    ),
  );

  const durationGrid = document.createElement("div");
  durationGrid.className = "field-grid two";
  durationGrid.append(
    field(
      "Duration",
      numberInput(
        selected.duration,
        (value) =>
          replaceSelectedAbility((ability) => {
            ability.duration = value;
          }),
        { min: 0 },
      ),
    ),
    field(
      "Targets",
      numberInput(
        selected.targets,
        (value) =>
          replaceSelectedAbility((ability) => {
            ability.targets = value;
          }),
        { min: 0 },
      ),
    ),
  );

  basics.append(
    basicGrid,
    textGrid,
    numbers,
    durationGrid,
    field(
      "Description",
      textArea(selected.description, (value) =>
        replaceSelectedAbility((ability) => {
          ability.description = value;
        }),
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
        numberInput(
          selected.shape.Radius,
          (value) =>
            replaceSelectedAbility((ability) => {
              ability.shape = { Radius: value };
            }),
          { step: 0.1 },
        ),
      ),
    );
  } else if (shapeKind === "Line") {
    const grid = document.createElement("div");
    grid.className = "field-grid two";
    grid.append(
      field(
        "Length",
        numberInput(
          selected.shape.Line.length,
          (value) =>
            replaceSelectedAbility((ability) => {
              ability.shape.Line.length = value;
            }),
          { step: 0.1 },
        ),
      ),
      field(
        "Thickness",
        numberInput(
          selected.shape.Line.thickness,
          (value) =>
            replaceSelectedAbility((ability) => {
              ability.shape.Line.thickness = value;
            }),
          { step: 0.1 },
        ),
      ),
    );
    shapeCard.append(grid);
  } else if (shapeKind === "Cone") {
    const grid = document.createElement("div");
    grid.className = "field-grid two";
    grid.append(
      field(
        "Angle",
        numberInput(
          selected.shape.Cone.angle,
          (value) =>
            replaceSelectedAbility((ability) => {
              ability.shape.Cone.angle = value;
            }),
          { step: 0.1 },
        ),
      ),
      field(
        "Radius",
        numberInput(
          selected.shape.Cone.radius,
          (value) =>
            replaceSelectedAbility((ability) => {
              ability.shape.Cone.radius = value;
            }),
          { step: 0.1 },
        ),
      ),
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
      replaceSelectedAbility((ability) => {
        ability.effects.splice(index, 1);
      }),
    );
    controls.append(kindSelect, removeButton);
    headerRow.append(title, controls);
    card.appendChild(headerRow);

    if (kind === "Heal") {
      const data = effect.Heal;
      const grid = document.createElement("div");
      grid.className = "field-grid three";
      grid.append(
        field(
          "Floor",
          numberInput(data.floor, (value) =>
            replaceSelectedAbility((ability) => {
              ability.effects[index].Heal.floor = value;
            }),
          ),
        ),
        field(
          "Ceiling",
          numberInput(data.ceiling, (value) =>
            replaceSelectedAbility((ability) => {
              ability.effects[index].Heal.ceiling = value;
            }),
          ),
        ),
        field(
          "Scaled With",
          selectInput(STAT_OPTIONS, data.scaled_with, (value) =>
            replaceSelectedAbility((ability) => {
              ability.effects[index].Heal.scaled_with = value;
            }),
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
        field(
          "Floor",
          numberInput(data.floor, (value) =>
            replaceSelectedAbility((ability) => {
              ability.effects[index].Damage.floor = value;
            }),
          ),
        ),
        field(
          "Ceiling",
          numberInput(data.ceiling, (value) =>
            replaceSelectedAbility((ability) => {
              ability.effects[index].Damage.ceiling = value;
            }),
          ),
        ),
        field(
          "Damage Type",
          selectInput(DAMAGE_TYPE_OPTIONS, data.damage_type, (value) =>
            replaceSelectedAbility((ability) => {
              ability.effects[index].Damage.damage_type = value;
            }),
          ),
        ),
        field(
          "Scaled With",
          selectInput(STAT_OPTIONS, data.scaled_with, (value) =>
            replaceSelectedAbility((ability) => {
              ability.effects[index].Damage.scaled_with = value;
            }),
          ),
        ),
      );
      const bottom = document.createElement("div");
      bottom.className = "field-grid two";
      bottom.append(
        field(
          "Defended With",
          selectInput(STAT_OPTIONS, data.defended_with, (value) =>
            replaceSelectedAbility((ability) => {
              ability.effects[index].Damage.defended_with = value;
            }),
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
        field(
          "Stat",
          selectInput(STAT_OPTIONS, data.stat, (value) =>
            replaceSelectedAbility((ability) => {
              ability.effects[index].Buff.stat = value;
            }),
          ),
        ),
        field(
          "Multiplier",
          numberInput(
            data.multiplier,
            (value) =>
              replaceSelectedAbility((ability) => {
                ability.effects[index].Buff.multiplier = value;
              }),
            { step: 0.05 },
          ),
        ),
        field(
          "Scaled With",
          selectInput(STAT_OPTIONS, data.scaled_with, (value) =>
            replaceSelectedAbility((ability) => {
              ability.effects[index].Buff.scaled_with = value;
            }),
          ),
        ),
        checkboxInput(
          Boolean(data.effects),
          (checked) =>
            replaceSelectedAbility((ability) => {
              ability.effects[index].Buff.effects = checked ? [] : null;
            }),
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

function buildDialogueEditor() {
  const container = document.createElement("section");
  container.className = "card section-stack";
  const selected =
    state.selectedDialogueIndex === null ? null : state.dialogues[state.selectedDialogueIndex];

  if (!selected) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = "Select a dialogue to edit or create a new one.";
    container.appendChild(empty);
    return container;
  }

  const header = document.createElement("div");
  header.className = "panel-header";
  header.innerHTML = `<div><p class="panel-kicker">Dialogue</p><h2>${selected.id}</h2></div>`;
  const actions = document.createElement("div");
  actions.className = "action-cluster";

  const duplicateButton = document.createElement("button");
  duplicateButton.textContent = "Duplicate";
  duplicateButton.addEventListener("click", () => {
    const duplicate = clone(selected);
    duplicate.id = uniqueDialogueId(state.dialogues);
    state.dialogues.push(duplicate);
    state.selectedDialogueIndex = state.dialogues.length - 1;
    setDirty(true);
    render();
  });

  const deleteButton = document.createElement("button");
  deleteButton.textContent = "Delete";
  deleteButton.className = "danger-button";
  deleteButton.addEventListener("click", () => {
    state.dialogues.splice(state.selectedDialogueIndex, 1);
    state.selectedDialogueIndex =
      state.dialogues.length === 0
        ? null
        : Math.min(state.selectedDialogueIndex, state.dialogues.length - 1);
    setDirty(true);
    render();
  });

  actions.append(duplicateButton, deleteButton);
  header.appendChild(actions);
  container.appendChild(header);

  const core = document.createElement("div");
  core.className = "editor-card section-stack";
  core.innerHTML = `<p class="panel-kicker">Core</p>`;
  const coreGrid = document.createElement("div");
  coreGrid.className = "field-grid two";
  coreGrid.append(
    field(
      "ID",
      textInput(selected.id, (value) =>
        replaceSelectedDialogue((dialogue) => {
          dialogue.id = value;
        }),
      ),
    ),
    field(
      "Speaker",
      textInput(selected.speaker, (value) =>
        replaceSelectedDialogue((dialogue) => {
          dialogue.speaker = value;
        }),
      ),
    ),
  );

  const nextOptions = ["", ...state.dialogues.map((dialogue) => dialogue.id)];
  core.append(
    coreGrid,
    field(
      "Text",
      textArea(selected.text, (value) =>
        replaceSelectedDialogue((dialogue) => {
          dialogue.text = value;
        }),
      ),
    ),
    field(
      "Next",
      (() => {
        const select = selectInput(nextOptions, selected.next ?? "", (value) =>
          replaceSelectedDialogue((dialogue) => {
            dialogue.next = value === "" ? null : value;
          }),
        );
        select.options[0].textContent = "None";
        return select;
      })(),
    ),
  );

  const relationCard = document.createElement("div");
  relationCard.className = "editor-card section-stack";
  relationCard.innerHTML = `<p class="panel-kicker">Flow</p>`;
  const incoming = state.dialogues
    .filter(
      (dialogue) =>
        dialogue.next === selected.id ||
        (dialogue.choices ?? []).some((choice) => choice.next === selected.id),
    )
    .map((dialogue) => dialogue.id);
  const outgoing = [
    ...(selected.next ? [selected.next] : []),
    ...(selected.choices ?? []).map((choice) => choice.next).filter(Boolean),
  ];
  const flowGrid = document.createElement("div");
  flowGrid.className = "field-grid two";
  flowGrid.append(
    field("Incoming", textInput(incoming.join(", "), () => {})),
    field("Outgoing", textInput(outgoing.join(", "), () => {})),
  );
  flowGrid.querySelectorAll("input").forEach((input) => {
    input.readOnly = true;
  });
  relationCard.appendChild(flowGrid);

  const choicesCard = document.createElement("div");
  choicesCard.className = "editor-card section-stack";
  const choicesHeader = document.createElement("div");
  choicesHeader.className = "choice-header";
  choicesHeader.innerHTML = `<div><p class="panel-kicker">Choices</p><h2>Branch Options</h2></div>`;
  const addChoiceButton = document.createElement("button");
  addChoiceButton.textContent = "Add Choice";
  addChoiceButton.addEventListener("click", () =>
    replaceSelectedDialogue((dialogue) => {
      if (!dialogue.choices) dialogue.choices = [];
      dialogue.choices.push({
        event: 0,
        text: "New choice",
        next: uniqueDialogueId(state.dialogues),
      });
    }),
  );
  choicesHeader.appendChild(addChoiceButton);
  choicesCard.appendChild(choicesHeader);

  if (!(selected.choices ?? []).length) {
    const empty = document.createElement("div");
    empty.className = "help-text";
    empty.textContent = "No branching choices on this node.";
    choicesCard.appendChild(empty);
  }

  (selected.choices ?? []).forEach((choice, index) => {
    const card = document.createElement("div");
    card.className = "subcard section-stack";
    const headerRow = document.createElement("div");
    headerRow.className = "choice-header";
    const title = document.createElement("strong");
    title.textContent = `Choice ${index + 1}`;
    const removeButton = document.createElement("button");
    removeButton.textContent = "Remove";
    removeButton.className = "danger-button";
    removeButton.addEventListener("click", () =>
      replaceSelectedDialogue((dialogue) => {
        dialogue.choices.splice(index, 1);
        if (dialogue.choices.length === 0) dialogue.choices = null;
      }),
    );
    headerRow.append(title, removeButton);
    card.appendChild(headerRow);

    const grid = document.createElement("div");
    grid.className = "field-grid three";
    const nextSelect = selectInput(
      state.dialogues.map((dialogue) => dialogue.id),
      choice.next ?? "",
      (value) =>
        replaceSelectedDialogue((dialogue) => {
          dialogue.choices[index].next = value === "" ? null : value;
        }),
    );
    grid.append(
      field(
        "Text",
        textInput(choice.text, (value) =>
          replaceSelectedDialogue((dialogue) => {
            dialogue.choices[index].text = value;
          }),
        ),
      ),
      field(
        "Event",
        numberInput(
          choice.event,
          (value) =>
            replaceSelectedDialogue((dialogue) => {
              dialogue.choices[index].event = value;
            }),
          { min: 0 },
        ),
      ),
      field("Next", nextSelect),
    );
    card.appendChild(grid);
    choicesCard.appendChild(card);
  });

  container.append(core, relationCard, choicesCard);
  return container;
}

function renderEditor() {
  elements.editorRoot.innerHTML = "";
  elements.editorRoot.appendChild(
    state.editor === "abilities" ? buildAbilityEditor() : buildDialogueEditor(),
  );
}

function renderValidationMessages() {
  const validation = activeValidationMessages();
  if (validation.length === 0) return;
  for (const message of validation) {
    state.messages.unshift({
      id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
      kind: message === "No issues detected." ? "success" : "error",
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
    state.dialogues = data;
    state.selectedDialogueIndex = data.length > 0 ? 0 : null;
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
  // Local backup of the JSON-shaped data. The server is the source of truth
  // for the canonical RON file.
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
    state.editor === "abilities"
      ? validateAbilities(state.abilities)
      : validateDialogues(state.dialogues);
  render();
  renderValidationMessages();
}

function addNewEntry() {
  if (state.editor === "abilities") {
    state.abilities.push(createDefaultAbility());
    state.selectedAbilityIndex = state.abilities.length - 1;
  } else {
    state.dialogues.push(createDefaultDialogue());
    state.selectedDialogueIndex = state.dialogues.length - 1;
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
