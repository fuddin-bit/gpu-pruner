In our shared cluster environments, users often spin up ephemeral experiments (either in their personal {user}-dev namespaces, or elsewhere) and often forget to spin them back down. This results in wasted resources and can block other important jobs (like CI tasks) from running.

We have an old project we can dust off to solve this problem: [https://github.com/wseaton/gpu-pruner](https://github.com/wseaton/gpu-pruner). Or, you can make a new one if you see fit\!

Note: this project needs to be revalidated against the current state of things, especially LLMInferenceServices (from KServe), but that is not super relevant since KServe is rarely tested in waldorf.

But first, this depends on a cluster monitoring stack being available

1) We’d like to start with coreweave-waldorf  
   1) Why: it is the most popular shared environment  
   2) the monitoring stack needs to be fixed there, gpu pruner needs a prometheus server to query the [nvidia DCGM GPU](https://github.com/NVIDIA/dcgm-exporter) metrics from  
2) We need a few enhancements to the setup

We need an UI dashboard that showcases:

current running workload 
all idle gpu workloads 
a list of the users consuming the most resources

we have some of this data already in the server logs, you can see them on waldorf via kubectl logs deployment/gpu-pruner --context coreweave-waldorf --namespace gpu-pruner-system