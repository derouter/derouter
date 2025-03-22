use std::collections::HashMap;

#[derive(Default)]
struct MyLogLineFilter {
	module_filters: HashMap<String, log::LevelFilter>,
}

impl MyLogLineFilter {
	pub fn add_module_filter(&mut self, module: &str, level: log::LevelFilter) {
		self.module_filters.insert(module.to_string(), level);
	}
}

impl flexi_logger::filter::LogLineFilter for MyLogLineFilter {
	fn write(
		&self,
		now: &mut flexi_logger::DeferredNow,
		record: &log::Record,
		log_line_writer: &dyn flexi_logger::filter::LogLineWriter,
	) -> std::io::Result<()> {
		if let Some(module_path) = record.module_path() {
			for (module_filter, level) in &self.module_filters {
				if module_path.starts_with(module_filter) {
					if record.level() <= *level {
						return log_line_writer.write(now, record);
					} else {
						return Ok(()); // Skip this log line.
					}
				}
			}
		}

		log_line_writer.write(now, record)
	}
}

pub fn setup_logger() -> eyre::Result<flexi_logger::LoggerHandle> {
	let mut my_filter = MyLogLineFilter::default();

	my_filter.add_module_filter("tracing::span", log::LevelFilter::Info);
	my_filter
		.add_module_filter("libp2p_quic::transport", log::LevelFilter::Info);
	my_filter.add_module_filter("multistream_select", log::LevelFilter::Info);
	my_filter.add_module_filter("quinn::connection", log::LevelFilter::Info);
	my_filter.add_module_filter("yamux::connection", log::LevelFilter::Info);
	my_filter.add_module_filter("libp2p_ping::handler", log::LevelFilter::Info);
	my_filter.add_module_filter("libp2p_swarm", log::LevelFilter::Info);
	my_filter.add_module_filter(
		"libp2p_gossipsub::behaviour",
		log::LevelFilter::Info,
	);

	Ok(flexi_logger::Logger::try_with_env()?
		.format(flexi_logger::colored_default_format)
		.filter(Box::new(my_filter))
		.log_to_stderr()
		.start()?)
}
