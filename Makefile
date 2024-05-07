deps:
	git clone https://github.com/starkware-libs/cairo.git \
	&& cd cairo \
	&& git checkout 447c6eb01364449dc67b676073646338c7e364ce \
	&& cd .. \
	&& mv cairo/corelib/ . \
	&& rm -rf cairo/