{{- define "ramzes.name" -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "ramzes.labels" -}}
app.kubernetes.io/name: ramzes
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "ramzes.selector" -}}
app.kubernetes.io/name: ramzes
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}
