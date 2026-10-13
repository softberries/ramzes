<script lang="ts">
	import { SEVERITIES } from '#lib/api/types.ts';
	import { COMMON_ZONES, DAYS, UNITS, type ShapingForm, type Span } from '#lib/shaping.ts';

	/** The policy being edited. Turn it into what the API takes with `formToShaping`. */
	let { form = $bindable() }: { form: ShapingForm } = $props();

	function toggleDay(day: string) {
		const days = form.quiet.days;
		form.quiet.days = days.includes(day) ? days.filter((d) => d !== day) : [...days, day];
	}
</script>

{#snippet span(id: string, label: string, value: Span, required: boolean)}
	<div>
		<label class="field-label" for={id}>{label}</label>
		<div class="flex gap-2">
			<input
				{id}
				class="input w-24"
				type="number"
				min="1"
				step="1"
				inputmode="numeric"
				{required}
				bind:value={value.amount}
			/>
			<select class="input w-32" aria-label="{label}: unit" bind:value={value.unit}>
				{#each UNITS as unit (unit.unit)}<option value={unit.unit}>{unit.label}</option>{/each}
			</select>
		</div>
	</div>
{/snippet}

<div class="flex flex-col gap-4">
	<div class="flex flex-col gap-3">
		<label class="flex items-center gap-2">
			<input type="checkbox" bind:checked={form.dedup.on} /> Count repeats of the same event instead of sending them again
		</label>
		{#if form.dedup.on}
			<div class="part grid gap-4 md:grid-cols-[auto_1fr]">
				{@render span('shape-dedup-window', 'For this long after the first one', form.dedup.window, true)}
				<div>
					<label class="field-label" for="shape-dedup-key">What makes two events the same</label>
					<input
						id="shape-dedup-key"
						class="input"
						placeholder="The event's dedup key, or its topic and labels"
						autocomplete="off"
						spellcheck="false"
						bind:value={form.dedup.key}
					/>
					<p class="hint mt-1">Optional. An expression such as labels.order or payload.host.</p>
				</div>
			</div>
		{/if}
	</div>

	<div class="flex flex-col gap-3">
		<label class="flex items-center gap-2">
			<input type="checkbox" bind:checked={form.group.on} /> Collect events and send them as one message
		</label>
		{#if form.group.on}
			<div class="part grid gap-4 md:grid-cols-[1fr_auto_auto]">
				<div>
					<label class="field-label" for="shape-group-by">Labels to group by</label>
					<input
						id="shape-group-by"
						class="input"
						placeholder="service, env"
						autocomplete="off"
						spellcheck="false"
						bind:value={form.group.by}
					/>
					<p class="hint mt-1">Separated by commas. Leave empty for one group per connection.</p>
				</div>
				{@render span('shape-group-wait', 'Wait before sending', form.group.wait, true)}
				<div>
					{@render span('shape-group-repeat', 'Send again while still open, every', form.group.repeat, false)}
					<p class="hint mt-1">Leave empty to send once.</p>
				</div>
			</div>
		{/if}
	</div>

	<div class="flex flex-col gap-3">
		<label class="flex items-center gap-2">
			<input type="checkbox" bind:checked={form.quiet.on} /> Hold messages during quiet hours and send one digest after
		</label>
		{#if form.quiet.on}
			<div class="part flex flex-col gap-4">
				<div class="grid grid-cols-2 gap-4 md:grid-cols-[minmax(0,16rem)_auto_auto]">
					<div class="col-span-2 md:col-span-1">
						<label class="field-label" for="shape-quiet-tz">Time zone</label>
						<input
							id="shape-quiet-tz"
							class="input"
							list="shape-zones"
							required
							autocomplete="off"
							spellcheck="false"
							bind:value={form.quiet.tz}
						/>
						<datalist id="shape-zones">
							{#each COMMON_ZONES as zone (zone)}<option value={zone}></option>{/each}
						</datalist>
					</div>
					<div>
						<label class="field-label" for="shape-quiet-start">From</label>
						<input id="shape-quiet-start" class="input" type="time" required bind:value={form.quiet.start} />
					</div>
					<div>
						<label class="field-label" for="shape-quiet-end">Until</label>
						<input id="shape-quiet-end" class="input" type="time" required bind:value={form.quiet.end} />
					</div>
				</div>
				<div>
					<span class="field-label" id="shape-quiet-days">On the nights starting</span>
					<div class="flex flex-wrap gap-1" role="group" aria-labelledby="shape-quiet-days">
						{#each DAYS as day (day.key)}
							{@const on = form.quiet.days.includes(day.key)}
							<button
								type="button"
								class="btn h-8 px-2.5 {on ? 'border-accent text-accent' : 'text-muted'}"
								aria-pressed={on}
								onclick={() => toggleDay(day.key)}
							>
								{day.label}
							</button>
						{/each}
					</div>
					<p class="hint mt-1">
						{form.quiet.days.length === 0 ? 'None chosen: every day.' : 'A quiet period belongs to the day it starts on.'}
					</p>
				</div>
				<div class="max-w-xs">
					<label class="field-label" for="shape-quiet-bypass">Send anyway at or above</label>
					<select id="shape-quiet-bypass" class="input" bind:value={form.quiet.bypass}>
						<option value="">Nothing: hold every severity</option>
						{#each SEVERITIES.slice(1) as severity (severity)}<option value={severity}>{severity}</option>{/each}
					</select>
				</div>
			</div>
		{/if}
	</div>

	<div class="flex flex-col gap-3">
		<label class="flex items-center gap-2">
			<input type="checkbox" bind:checked={form.rate.on} /> Limit how many messages are sent
		</label>
		{#if form.rate.on}
			<div class="part flex flex-wrap gap-4">
				<div>
					<label class="field-label" for="shape-rate-max">At most this many</label>
					<input
						id="shape-rate-max"
						class="input w-24"
						type="number"
						min="1"
						step="1"
						inputmode="numeric"
						required
						bind:value={form.rate.max}
					/>
				</div>
				{@render span('shape-rate-window', 'In every', form.rate.window, true)}
				<p class="hint basis-full">The rest are held and sent as one summary when the window ends.</p>
			</div>
		{/if}
	</div>

	<label class="flex items-center gap-2">
		<input type="checkbox" bind:checked={form.notifyResolved} /> Tell the target when it is resolved
	</label>
</div>
