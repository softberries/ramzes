import type { components } from './schema';

type Schemas = components['schemas'];

export type Severity = Schemas['Severity'];
export type EventStatus = Schemas['EventStatus'];
export type RamzesEvent = Schemas['Event'];
export type EventRow = Schemas['EventRow'];
export type EventPage = Schemas['EventPage'];
export type Journey = Schemas['Journey'];
export type JourneyNotification = Schemas['JourneyNotification'];
export type JourneyDelivery = Schemas['JourneyDelivery'];
export type Delivery = Schemas['Delivery'];
export type DeliveryState = Schemas['DeliveryState'];
export type Attempt = Schemas['Attempt'];
export type FailureRow = Schemas['FailureRow'];
export type Source = Schemas['SourceView'];
export type SourceCreated = Schemas['SourceCreated'];
export type SourceRequest = Schemas['SourceRequest'];
export type ConnectorDescriptor = Schemas['ConnectorDescriptor'];
export type ConnectorField = Schemas['Field'];
export type ConnectionView = Schemas['ConnectionView'];
/**
 * What a connection's test message came back with. Written by hand: the API description names both
 * this and the route tester's answer `TestResult`, and the generated type is the route tester's.
 */
export interface TestResult {
	ok: boolean;
	status?: number | null;
	detail?: string | null;
}
export type Route = Schemas['RouteView'];
export type RouteInput = Schemas['RouteInput'];
export type RouteMatch = Schemas['RouteMatch'];
export type LabelCond = Schemas['LabelCond'];
export type Target = Schemas['Target'];
export type Settings = Schemas['Settings'];
export type UserView = Schemas['UserView'];
export type ApiToken = Schemas['ApiToken'];
export type TokenCreated = Schemas['TokenCreated'];
export type Me = Schemas['Me'];
export type ConnectionHealth = Schemas['ConnectionHealth'];
export type DefinitionRow = Schemas['DefinitionRow'];
export type SourceKind = Schemas['SourceKind'];
export type NewEvent = Schemas['NewEvent'];
export type MappingPreview = Schemas['MappingPreview'];
export type Template = Schemas['Template'];
export type TemplatePreview = Schemas['TemplatePreview'];
export type RequestPreview = Schemas['Preview'];
export type MessagePreview = Schemas['MessagePreview'];
export type RouteTest = Schemas['TestResult'];
export type RouteResult = Schemas['RouteResult'];
export type TargetResult = Schemas['TargetResult'];
export type RevisionMeta = Schemas['RevisionMeta'];
export type RevisionView = Schemas['RevisionView'];

export const SEVERITIES: Severity[] = ['debug', 'info', 'warning', 'error', 'critical'];

/** What became of an event, as the feed shows it. */
export type Outcome = 'pending' | 'delivered' | 'failed' | 'unrouted';
export const OUTCOMES: Outcome[] = ['pending', 'delivered', 'failed', 'unrouted'];
