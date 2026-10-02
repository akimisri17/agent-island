// Groups model ids by maker and size, from the name alone. Names come from the
// agents' own logs, so this has to tolerate dates, suffixes, and router names.

const MAKERS = [
  [/claude|opus|sonnet|haiku|fable/i, 'Anthropic'],
  [/^(gpt|o\d|chatgpt|codex)|openai/i, 'OpenAI'],
  [/grok/i, 'xAI'],
  [/gemini|gemma/i, 'Google'],
  [/qwen/i, 'Alibaba'],
  [/deepseek/i, 'DeepSeek'],
  [/llama/i, 'Meta'],
  [/mistral|codestral|devstral/i, 'Mistral'],
  [/kimi|moonshot/i, 'Moonshot'],
  [/glm|zhipu/i, 'Zhipu'],
  [/^cursor|^composer/i, 'Cursor'],
];

export function makerOf(model) {
  // Router prefixes like "openrouter/x-ai/grok-4" or "anthropic/claude-…"
  const name = String(model).split('/').pop();
  for (const [re, maker] of MAKERS) if (re.test(name) || re.test(model)) return maker;
  return 'Other';
}

const SMALL = /haiku|mini|nano|flash|lite|small|tiny|-fast\b/i;
const LARGE = /opus|fable|ultra|\bo3\b|-pro\b|-high\b|max\b|grok-\d(\.\d+)?$/i;

export function tierOf(model) {
  if (SMALL.test(model)) return 'small';
  if (LARGE.test(model)) return 'large';
  return 'mid';
}

export function prettyModel(m) {
  return String(m).split('/').pop().replace(/^claude-/, '').replace(/-(\d{8})$/, '');
}
