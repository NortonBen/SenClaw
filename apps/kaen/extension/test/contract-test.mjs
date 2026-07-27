#!/usr/bin/env node
/**
 * Kaen Vocabulary Helper — API contract test.
 *
 * Simulates exactly the calls the extension's background.js makes against a
 * REAL running Kaen backend (no mocks). Requires Node >= 18 (global fetch).
 *
 * Usage:
 *   cargo build -p kaen                                    # from the SemaClaw repo root
 *   PORT=4505 KAEN_DATA_DIR=$(mktemp -d) ./target/debug/kaen &
 *   node apps/kaen/extension/test/contract-test.mjs http://localhost:4505/api
 *
 * Exits 0 if every step passes, 1 otherwise.
 */

const BASE = (process.argv[2] || 'http://localhost:4500/api').replace(/\/+$/, '');

let passed = 0;
let failed = 0;

function ok(step, detail = '') {
    passed++;
    console.log(`  PASS  ${step}${detail ? ' — ' + detail : ''}`);
}

function fail(step, detail = '') {
    failed++;
    console.error(`  FAIL  ${step}${detail ? ' — ' + detail : ''}`);
}

function assert(cond, step, detail = '') {
    if (cond) ok(step, detail);
    else fail(step, detail);
    return cond;
}

async function jsonFetch(url, options = {}) {
    const res = await fetch(url, {
        headers: { 'Content-Type': 'application/json' },
        ...options
    });
    let body = null;
    try { body = await res.json(); } catch { /* non-JSON */ }
    return { res, body };
}

async function main() {
    console.log(`Contract test against ${BASE}\n`);

    // ---- 1. Health check (what the popup's "connected" state uses) ----
    console.log('[1] GET /status (health-check)');
    const { res: hRes, body: health } = await jsonFetch(`${BASE}/status`);
    assert(hRes.ok, 'status responds 200', `HTTP ${hRes.status}`);
    assert(health?.ok === true, 'body.ok === true', JSON.stringify(health));
    assert(health?.name === 'kaen', 'body.name === "kaen"');

    // ---- 2. Create lesson (CREATE_LESSON message) ----
    console.log('\n[2] POST /lessons { title: "Extension Test" }');
    const { res: cRes, body: lesson } = await jsonFetch(`${BASE}/lessons`, {
        method: 'POST',
        body: JSON.stringify({ title: 'Extension Test', description: 'Created by contract-test.mjs' })
    });
    assert(cRes.ok, 'create lesson responds 200', `HTTP ${cRes.status}`);
    const lessonId = lesson?.id;
    if (!assert(typeof lessonId === 'string' && lessonId.length > 0, 'lesson has id', String(lessonId))) {
        return finish();
    }
    assert(lesson?.title === 'Extension Test', 'lesson title echoed');

    // ---- 3. List lessons (GET_LESSONS message) — envelope, NOT a bare array ----
    console.log('\n[3] GET /lessons?limit=100 (envelope shape)');
    const { res: lRes, body: page } = await jsonFetch(`${BASE}/lessons?limit=100`);
    assert(lRes.ok, 'list lessons responds 200', `HTTP ${lRes.status}`);
    assert(!Array.isArray(page), 'response is an envelope, not a bare array');
    assert(Array.isArray(page?.lessons), 'envelope has lessons[]');
    assert(typeof page?.total === 'number', 'envelope has total', `total=${page?.total}`);
    const found = (page?.lessons || []).find(l => l.id === lessonId);
    assert(!!found, 'created lesson appears in list', found ? `cardCount=${found.cardCount}` : '');
    assert(found && typeof found.cardCount === 'number', 'lesson has cardCount (camelCase)');

    // search param, as the lesson dropdown uses it
    const { body: searchPage } = await jsonFetch(`${BASE}/lessons?limit=100&search=${encodeURIComponent('Extension Test')}`);
    assert((searchPage?.lessons || []).some(l => l.id === lessonId), 'search=Extension Test finds it');

    // ---- 4. Save card (SAVE_TO_LESSON message) — full field mapping ----
    console.log('\n[4] POST /lessons/:id/cards (full card)');
    const cardData = {
        word: 'serendipity',
        ipa: '/ˌser.ənˈdɪp.ə.ti/',
        partOfSpeech: 'noun',
        examples: [
            'A fortunate stroke of serendipity brought them together.',
            'Meeting her was pure serendipity.'
        ],
        explain: 'the fact of finding interesting or valuable things by chance',
        meanings: { vi: 'sự tình cờ may mắn' }
    };
    const { res: sRes, body: card } = await jsonFetch(`${BASE}/lessons/${lessonId}/cards`, {
        method: 'POST',
        body: JSON.stringify(cardData)
    });
    assert(sRes.ok, 'save card responds 200', `HTTP ${sRes.status}`);
    assert(typeof card?.id === 'string' && card.id.length > 0, 'card has id', String(card?.id));
    assert(card?.word === 'serendipity', 'card.word round-trips');

    // ---- 5. Read back cards and verify every field survived ----
    console.log('\n[5] GET /lessons/:id/cards (verify saved fields)');
    const { res: gRes, body: session } = await jsonFetch(`${BASE}/lessons/${lessonId}/cards`);
    assert(gRes.ok, 'get lesson cards responds 200', `HTTP ${gRes.status}`);
    assert(Array.isArray(session?.cards), 'response has cards[]');
    const saved = (session?.cards || []).find(c => c.word === 'serendipity');
    if (assert(!!saved, 'saved card found by word')) {
        assert(saved.ipa === cardData.ipa, 'ipa preserved', String(saved.ipa));
        assert(saved.partOfSpeech === cardData.partOfSpeech, 'partOfSpeech preserved (camelCase)', String(saved.partOfSpeech));
        assert(saved.explain === cardData.explain, 'explain preserved (definition mapping)');
        assert(Array.isArray(saved.examples) && saved.examples[0] === cardData.examples[0],
            'examples preserved', `count=${Array.isArray(saved.examples) ? saved.examples.length : 'n/a'}`);
        assert(saved.meanings && saved.meanings.vi === cardData.meanings.vi,
            'meanings.vi preserved (translation mapping)', String(saved.meanings?.vi));
    }
    assert(session?.lesson?.title === 'Extension Test', 'lesson title in cards response');

    // cardCount should now reflect the new card
    const { body: page2 } = await jsonFetch(`${BASE}/lessons?limit=100`);
    const after = (page2?.lessons || []).find(l => l.id === lessonId);
    assert(after?.cardCount === 1, 'lesson cardCount incremented to 1', `cardCount=${after?.cardCount}`);

    // ---- 6. (Optional source) GET /dictionary/lookup — used as lookup fallback ----
    console.log('\n[6] GET /dictionary/lookup?word=hello&targetLang=vi (lookup fallback shape)');
    const { res: dRes, body: dict } = await jsonFetch(`${BASE}/dictionary/lookup?word=hello&targetLang=vi`);
    assert(dRes.ok, 'dictionary lookup responds 200', `HTTP ${dRes.status}`);
    assert(dict?.word === 'hello', 'dict.word echoed');
    for (const key of ['ipa', 'partOfSpeech', 'definition', 'examples', 'audioUrl', 'translation']) {
        assert(key in (dict || {}), `dict has "${key}" key`);
    }

    finish();
}

function finish() {
    console.log(`\n${passed} passed, ${failed} failed`);
    process.exit(failed > 0 ? 1 : 0);
}

main().catch(err => {
    console.error('\nFATAL:', err.message);
    console.error('Is the Kaen backend running at', BASE, '?');
    process.exit(1);
});
