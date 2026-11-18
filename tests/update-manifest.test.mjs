// 업데이트 매니페스트 만들기(scripts/update-manifest.mjs) 시험. 저장소 뿌리에서 `node tests/update-manifest.test.mjs`.
import assert from 'node:assert/strict';
import { compareVersions, mergeReleases, ASSETS } from '../scripts/update-manifest.mjs';

assert.ok(compareVersions('0.10.0', '0.9.9') > 0);
assert.equal(compareVersions('v0.2.1', '0.2.1'), 0);
assert.ok(compareVersions('0.2.1', '0.3.0') < 0);

const old = { releases: [{ version: '0.2.1' }, { version: '0.2.0' }, { version: '0.1.0' }] };
assert.deepEqual(mergeReleases(old, { version: '0.3.0' }, 3).map((r) => r.version), ['0.3.0', '0.2.1', '0.2.0']);
// 같은 판을 다시 올리면 바꿔 끼운다.
assert.deepEqual(mergeReleases(old, { version: '0.2.1', notes: 'new' }, 3).map((r) => [r.version, r.notes ?? '']), [['0.2.1', 'new'], ['0.2.0', ''], ['0.1.0', '']]);
assert.deepEqual(mergeReleases(null, { version: '1.0.0' }, 3).map((r) => r.version), ['1.0.0']);

// 앱(src-tauri/src/update.rs 의 asset_key)이 찾는 키가 있어야 한다.
const keys = ASSETS.map(([key]) => key);
assert.ok(keys.includes('macos-arm64') && keys.includes('windows-x64-setup'));

console.log('update-manifest ok');
