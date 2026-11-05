import m from 'mithril';
import { t } from '../../i18n';
import type { KnowledgeSearchPlan, KnowledgeSearchResult } from '../../models';
import { fallbackReasonLabel, modeLabel, priorityLabel, reasonLabel } from './knowledge-labels';

/** The previewed search plan: scope, planned queries, and what was excluded. */
export function knowledgePlanBody(plan: KnowledgeSearchPlan): m.Vnode {
  return m('.fm-knowledge-plan-body', [
    m('p', t('knowledgeSearch', 'planVersion', { version: plan.version })),
    m('p', t('knowledgeSearch', 'planScope', { scope: plan.scopeLabel })),
    m(
      'p',
      t('knowledgeSearch', 'planSources', {
        count: plan.authorizedSources,
      }),
    ),
    m(
      'p',
      t('knowledgeSearch', 'planSearches', {
        count: plan.searches.length,
      }),
    ),
    plan.omittedSearches === 0
      ? undefined
      : m(
          'p.fm-knowledge-warning',
          t('knowledgeSearch', 'planOmitted', {
            count: plan.omittedSearches,
          }),
        ),
    m(
      'ol.fm-knowledge-planned-searches',
      plan.searches.map((planned, index) =>
        m('li', { key: `planned-${index}` }, [
          m('code', planned.text),
          m('small.fm-knowledge-hint', ` · ${priorityLabel(planned.priority)}`),
          m(
            'ul',
            planned.reasons.map((reason, reasonIndex) =>
              m('li', { key: `planned-${index}-reason-${reasonIndex}` }, reasonLabel(reason)),
            ),
          ),
        ]),
      ),
    ),
    plan.excludedFromRetrieval.length === 0
      ? undefined
      : m('.fm-knowledge-excluded', [
          m('h4', t('knowledgeSearch', 'excludedFromRetrieval')),
          m(
            'ul',
            plan.excludedFromRetrieval.map((field, index) =>
              m(
                'li',
                { key: `plan-excluded-${index}` },
                t('knowledgeSearch', 'excludedField', {
                  field: field.field,
                  value: field.value,
                }),
              ),
            ),
          ),
        ]),
  ]);
}

/** Route, coverage and withheld-source details of a finished search. */
export function knowledgeResultDetails(result: KnowledgeSearchResult): m.Vnode {
  const route = result.route;
  return m('details.fm-knowledge-result-details', [
    m('summary', t('knowledgeSearch', 'searchDetails')),
    route === undefined
      ? undefined
      : m(
          route.fallbackReason == null ? 'p' : 'p.fm-knowledge-warning',
          route.fallbackReason == null
            ? t('knowledgeSearch', 'routeApplied', {
                route: modeLabel(route.applied),
              })
            : t('knowledgeSearch', 'routeFallback', {
                requested: modeLabel(route.requested),
                applied: modeLabel(route.applied),
                reason: fallbackReasonLabel(route.fallbackReason),
              }),
        ),
    m(
      'p',
      result.coverage.indexed == null
        ? t('knowledgeSearch', 'coverageUnknownIndexed', {
            eligible: result.coverage.eligible,
            fingerprinted: result.coverage.fingerprinted,
            unavailable: result.coverage.unavailable,
          })
        : t('knowledgeSearch', 'coverage', {
            indexed: result.coverage.indexed,
            eligible: result.coverage.eligible,
            fingerprinted: result.coverage.fingerprinted,
            unavailable: result.coverage.unavailable,
          }),
    ),
    result.coverage.partial
      ? m('p.fm-knowledge-warning', t('knowledgeSearch', 'coveragePartial'))
      : undefined,
    result.coverage.scopeIsExact
      ? undefined
      : m('p.fm-knowledge-warning', t('knowledgeSearch', 'coverageInexact')),
    result.withheldUnauthorized === 0
      ? undefined
      : m(
          'p.fm-knowledge-warning',
          t('knowledgeSearch', 'withheldUnauthorized', {
            count: result.withheldUnauthorized,
          }),
        ),
  ]);
}
