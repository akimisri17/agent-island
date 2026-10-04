import { test } from 'node:test';
import assert from 'node:assert/strict';
import { icon, ICONS, sprite } from '../ui/icons.js';

test('icons: one family, every icon in the sprite, helper refers to it', () => {
  for (const name of ['clock', 'sun', 'branch', 'chart', 'more', 'back', 'arrow', 'copy', 'spark', 'down', 'term', 'trash', 'play', 'close', 'cal', 'history', 'book', 'plus', 'pencil', 'check']) {
    assert.ok(ICONS.includes(name), name);
    assert.match(sprite(), new RegExp(`<symbol id="i-${name}" viewBox="0 0 24 24">`));
  }
  assert.equal(icon('more'), '<svg class="i" aria-hidden="true"><use href="#i-more"></use></svg>');
  assert.equal(icon('back', 's'), '<svg class="i s" aria-hidden="true"><use href="#i-back"></use></svg>');
  assert.throws(() => icon('gear'), /unknown icon/);
});
