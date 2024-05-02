deps:
	git clone https://github.com/starkware-libs/cairo.git \
	&& cd cairo \
	&& git checkout v2.6.3 \
	&& cd .. \
	&& mv cairo/corelib/ . \
	&& rm -rf cairo/