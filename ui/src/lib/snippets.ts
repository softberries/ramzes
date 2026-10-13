/** A curl command that sends one sample event to this instance. */
export function curlForEvent(origin: string, key: string = '<source key>'): string {
	return [
		`curl -X POST ${origin}/v1/events \\`,
		`  -H "Authorization: Bearer ${key}" \\`,
		`  -H "Content-Type: application/json" \\`,
		`  -d '{"topic": "ramzes.hello", "title": "Hello from my service", "severity": "info"}'`
	].join('\n');
}
